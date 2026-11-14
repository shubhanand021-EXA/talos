//! GEPA skill lifecycle IPC: validated/published/deprecated/archived
//! transitions, usage statistics, version history with diffs, and the
//! periodic evaluation loop trigger.
//!
//! Promotion stays explicit and human-gated (`validate_skill`,
//! `publish_skill`); deprecation can also arrive from the GEPA loop
//! (sustained failure, staleness). Every transition emits a typed event;
//! usage counters and confidence scores ride on `list_skills`/`get_skill`
//! via the extended [`crate::memory::SkillDto`].

use talos_common::TalosError;

use crate::commands::IpcError;
use crate::events::{
    emit_talos_event, SKILL_ARCHIVED, SKILL_DEPRECATED, SKILL_EVALUATION_COMPLETED,
    SKILL_PUBLISHED, SKILL_VALIDATED,
};
use crate::memory::SkillDto;
use crate::state::AppState;

fn check_skill_id(skill_id: &str) -> Result<String, IpcError> {
    if skill_id.trim().is_empty() || skill_id.len() > 128 {
        return Err(IpcError::from(TalosError::InvalidId {
            what: "skill_id",
            reason: "must be 1..=128 chars".to_owned(),
        }));
    }
    Ok(skill_id.to_owned())
}

async fn transition_inner(
    state: &AppState,
    app: Option<&tauri::AppHandle>,
    skill_id: String,
    to: &'static str,
    event: crate::events::TalosEventName,
) -> Result<SkillDto, IpcError> {
    let skill_id = check_skill_id(&skill_id)?;
    let row = evidence_store::update_skill_status(&state.db, &skill_id, to).await?;
    if let Some(handle) = app {
        let _ = emit_talos_event(
            handle,
            event,
            serde_json::json!({
                "skillId": row.id,
                "status": row.status,
                "checkedAt": talos_common::now_iso(),
            }),
        );
    }
    Ok(SkillDto::from(row))
}

/// `get_skill` — full skill document (procedure text included).
pub async fn get_skill_inner(state: &AppState, skill_id: String) -> Result<SkillDto, IpcError> {
    let skill_id = check_skill_id(&skill_id)?;
    let row = evidence_store::get_skill(&state.db, &skill_id)
        .await?
        .ok_or_else(|| {
            IpcError::from(TalosError::InvalidId {
                what: "skill_id",
                reason: "skill does not exist".to_owned(),
            })
        })?;
    Ok(SkillDto::from(row))
}

/// `validate_skill` — `quarantined` → `validated` (human approval).
pub async fn validate_skill_inner(
    state: &AppState,
    app: Option<&tauri::AppHandle>,
    skill_id: String,
) -> Result<SkillDto, IpcError> {
    transition_inner(state, app, skill_id, "validated", SKILL_VALIDATED).await
}

/// `publish_skill` — `validated` → `published` (retrievable in assembly).
pub async fn publish_skill_inner(
    state: &AppState,
    app: Option<&tauri::AppHandle>,
    skill_id: String,
) -> Result<SkillDto, IpcError> {
    transition_inner(state, app, skill_id, "published", SKILL_PUBLISHED).await
}

/// `deprecate_skill` — `published` → `deprecated` (stale or failing).
pub async fn deprecate_skill_inner(
    state: &AppState,
    app: Option<&tauri::AppHandle>,
    skill_id: String,
) -> Result<SkillDto, IpcError> {
    transition_inner(state, app, skill_id, "deprecated", SKILL_DEPRECATED).await
}

/// `archive_skill` — any live status → `archived` (terminal).
pub async fn archive_skill_inner(
    state: &AppState,
    app: Option<&tauri::AppHandle>,
    skill_id: String,
) -> Result<SkillDto, IpcError> {
    transition_inner(state, app, skill_id, "archived", SKILL_ARCHIVED).await
}

/// Usage statistics shaped for the TS `SkillStats` schema (`camelCase`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillStatsDto {
    /// Skill identifier.
    pub skill_id: String,
    /// Total recorded applications.
    pub usage_count: i64,
    /// Applications on verified completions.
    pub success_count: i64,
    /// Applications on other outcomes.
    pub failure_count: i64,
    /// Running success rate in [0, 1].
    pub success_rate: f64,
    /// Confidence score in [0, 1].
    pub confidence_score: f64,
    /// Most recent use, if any.
    pub last_used_at: Option<String>,
    /// Below-threshold flag for human review.
    pub needs_review: bool,
}

/// `get_skill_stats` — usage/success/failure statistics for one skill.
pub async fn get_skill_stats_inner(
    state: &AppState,
    skill_id: String,
) -> Result<SkillStatsDto, IpcError> {
    let skill_id = check_skill_id(&skill_id)?;
    let row = evidence_store::get_skill(&state.db, &skill_id)
        .await?
        .ok_or_else(|| {
            IpcError::from(TalosError::InvalidId {
                what: "skill_id",
                reason: "skill does not exist".to_owned(),
            })
        })?;
    Ok(SkillStatsDto {
        skill_id: row.id.clone(),
        usage_count: row.usage_count,
        success_count: row.success_count,
        failure_count: row.failure_count,
        success_rate: evidence_store::success_rate(&row),
        confidence_score: row.confidence_score,
        last_used_at: row.last_used_at.clone(),
        needs_review: evidence_store::needs_review(&row),
    })
}

/// `list_skill_versions` — a skill plus every improved version, oldest first.
pub async fn list_skill_versions_inner(
    state: &AppState,
    skill_id: String,
) -> Result<Vec<SkillDto>, IpcError> {
    let skill_id = check_skill_id(&skill_id)?;
    Ok(evidence_store::list_skill_versions(&state.db, &skill_id)
        .await?
        .into_iter()
        .map(SkillDto::from)
        .collect())
}

/// One diff line shaped for the TS schema (`camelCase`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillDiffLineDto {
    /// `same` | `removed` | `added`.
    pub kind: String,
    /// Line text.
    pub text: String,
}

/// `diff_skill_versions` — deterministic line diff between two skill
/// procedure texts (by skill id; versions addressable by their row ids).
pub async fn diff_skill_versions_inner(
    state: &AppState,
    from_id: String,
    to_id: String,
) -> Result<Vec<SkillDiffLineDto>, IpcError> {
    let from_id = check_skill_id(&from_id)?;
    let to_id = check_skill_id(&to_id)?;
    let missing = || {
        IpcError::from(TalosError::InvalidId {
            what: "skill_id",
            reason: "skill does not exist".to_owned(),
        })
    };
    let from = evidence_store::get_skill(&state.db, &from_id)
        .await?
        .ok_or_else(missing)?;
    let to = evidence_store::get_skill(&state.db, &to_id)
        .await?
        .ok_or_else(missing)?;
    Ok(
        evidence_store::skill_diff(&from.procedure_text, &to.procedure_text)
            .into_iter()
            .map(|line| SkillDiffLineDto {
                kind: line.kind,
                text: line.text,
            })
            .collect(),
    )
}

/// Trigger outcome shaped for the TS schema (`camelCase`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvaluationTriggerDto {
    /// Always true: the check ran.
    pub started: bool,
    /// Whether the cadence was due (false = watermark already current).
    pub due: bool,
    /// Total tool receipts observed at trigger time.
    pub receipt_count: i64,
}

/// Run one GEPA pass in the background and emit `skillEvaluationCompleted`
/// (or log and skip emission when no app handle is available, e.g. tests).
/// Never blocks the caller: fallible work stays inside the spawned task.
pub fn run_gepa_background(db: evidence_store::EvidenceDb, app: Option<tauri::AppHandle>) {
    tokio::spawn(async move {
        let config = skill_extractor::GepaConfig::default();
        match skill_extractor::evaluate(&db, &config).await {
            Ok(report) => {
                if let Some(handle) = app.as_ref() {
                    let _ = emit_talos_event(
                        handle,
                        SKILL_EVALUATION_COMPLETED,
                        serde_json::json!({
                            "patternsFound": report.patterns_found,
                            "skillsProposed": report.skills_proposed,
                            "skillsUsed": report.skills_used.len(),
                            "skillsDeprecated": report.skills_deprecated,
                            "needsReview": report.needs_review,
                            "evidenceId": report.evidence_id,
                            "checkedAt": talos_common::now_iso(),
                        }),
                    );
                }
            }
            Err(err) => {
                eprintln!("[talos] GEPA evaluation failed: {err}");
            }
        }
    });
}

/// `trigger_skill_evaluation` — check the cadence watermark; when due, spawn
/// the evaluation in the background and return immediately (non-blocking).
pub async fn trigger_skill_evaluation_inner(
    state: &AppState,
    app: Option<tauri::AppHandle>,
    every_n_calls: Option<i64>,
) -> Result<EvaluationTriggerDto, IpcError> {
    let mut config = skill_extractor::GepaConfig::default();
    if let Some(n) = every_n_calls {
        if !(1..=1000).contains(&n) {
            return Err(IpcError::from(TalosError::InvalidId {
                what: "every_n_calls",
                reason: "must be 1..=1000".to_owned(),
            }));
        }
        config.every_n_calls = n;
    }
    let receipt_count = evidence_store::count_evidence_by_kind(&state.db, "tool_receipt").await?;
    let due = skill_extractor::evaluation_due(&state.db, &config).await?;
    if due {
        run_gepa_background(state.db.clone(), app);
    }
    Ok(EvaluationTriggerDto {
        started: true,
        due,
        receipt_count,
    })
}

/// Emit `skillCreated` when the runtime hook distilled a new skill for
/// `task_id` after `known_id` was observed (call before/after execution).
/// Silent when nothing new appeared.
pub async fn emit_skill_created_if_new(
    app: &tauri::AppHandle,
    db: &evidence_store::EvidenceDb,
    task_id: &str,
    known_id: Option<&str>,
) {
    let current = evidence_store::latest_skill_for_task(db, task_id)
        .await
        .ok()
        .flatten();
    let is_new = match (current, known_id) {
        (Some(row), previous) => Some(row.id.as_str()) != previous,
        (None, _) => false,
    };
    if is_new {
        if let Ok(Some(row)) = evidence_store::latest_skill_for_task(db, task_id).await {
            let _ = emit_talos_event(
                app,
                crate::events::SKILL_CREATED,
                serde_json::json!({
                    "skillId": row.id,
                    "status": row.status,
                    "checkedAt": talos_common::now_iso(),
                }),
            );
        }
    }
}

/// `get_skill` — full skill document (procedure text included).
#[tauri::command]
pub async fn get_skill(
    state: tauri::State<'_, AppState>,
    skill_id: String,
) -> Result<SkillDto, IpcError> {
    get_skill_inner(&state, skill_id).await
}

/// `validate_skill` — `quarantined` → `validated`.
#[tauri::command]
pub async fn validate_skill(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    skill_id: String,
) -> Result<SkillDto, IpcError> {
    validate_skill_inner(&state, Some(&app), skill_id).await
}

/// `publish_skill` — `validated` → `published`.
#[tauri::command]
pub async fn publish_skill(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    skill_id: String,
) -> Result<SkillDto, IpcError> {
    publish_skill_inner(&state, Some(&app), skill_id).await
}

/// `deprecate_skill` — `published` → `deprecated`.
#[tauri::command]
pub async fn deprecate_skill(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    skill_id: String,
) -> Result<SkillDto, IpcError> {
    deprecate_skill_inner(&state, Some(&app), skill_id).await
}

/// `archive_skill` — any live status → `archived`.
#[tauri::command]
pub async fn archive_skill(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    skill_id: String,
) -> Result<SkillDto, IpcError> {
    archive_skill_inner(&state, Some(&app), skill_id).await
}

/// `get_skill_stats` — usage/success/failure statistics for one skill.
#[tauri::command]
pub async fn get_skill_stats(
    state: tauri::State<'_, AppState>,
    skill_id: String,
) -> Result<SkillStatsDto, IpcError> {
    get_skill_stats_inner(&state, skill_id).await
}

/// `list_skill_versions` — a skill plus every improved version.
#[tauri::command]
pub async fn list_skill_versions(
    state: tauri::State<'_, AppState>,
    skill_id: String,
) -> Result<Vec<SkillDto>, IpcError> {
    list_skill_versions_inner(&state, skill_id).await
}

/// `diff_skill_versions` — line diff between two skill procedures.
#[tauri::command]
pub async fn diff_skill_versions(
    state: tauri::State<'_, AppState>,
    from_id: String,
    to_id: String,
) -> Result<Vec<SkillDiffLineDto>, IpcError> {
    diff_skill_versions_inner(&state, from_id, to_id).await
}

/// `trigger_skill_evaluation` — run the GEPA loop in the background when due.
#[tauri::command]
pub async fn trigger_skill_evaluation(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    every_n_calls: Option<i64>,
) -> Result<EvaluationTriggerDto, IpcError> {
    trigger_skill_evaluation_inner(&state, Some(app), every_n_calls).await
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_state() -> AppState {
        AppState::new_in_memory()
            .await
            .expect("in-memory state builds")
    }

    async fn seed_skill(state: &AppState, id: &str) {
        evidence_store::create_skill(
            &state.db,
            evidence_store::NewSkill {
                id: id.to_owned(),
                name: "read then hash".to_owned(),
                description: "Read a file and hash it.".to_owned(),
                procedure_text: "1. Call `fs.read`. 2. Record the hash.".to_owned(),
                source_task_id: None,
                version: "1".to_owned(),
                test_fixture_ref: None,
                required_tools: vec!["fs.read".to_owned()],
                permission_ceiling: "low".to_owned(),
            },
        )
        .await
        .expect("create");
    }

    #[test]
    fn bisect_placeholder() {
        let _ = (test_state as fn() -> _, seed_skill as fn(_, _) -> _);
    }

    #[tokio::test]
    async fn lifecycle_transitions_emit_and_validate() {
        let state = test_state().await;
        seed_skill(&state, "skill-life").await;
        let fetched = get_skill_inner(&state, "skill-life".to_owned())
            .await
            .expect("get");
        assert!(fetched.procedure_text.contains("fs.read"));
        assert_eq!(fetched.usage_count, 0);
        let validated = validate_skill_inner(&state, None, "skill-life".to_owned())
            .await
            .expect("validate");
        assert_eq!(validated.status, "validated");
        // No skips.
        assert!(publish_skill_inner(&state, None, "missing".to_owned())
            .await
            .is_err());
        let published = publish_skill_inner(&state, None, "skill-life".to_owned())
            .await
            .expect("publish");
        assert_eq!(published.status, "published");
        let deprecated = deprecate_skill_inner(&state, None, "skill-life".to_owned())
            .await
            .expect("deprecate");
        assert_eq!(deprecated.status, "deprecated");
        let archived = archive_skill_inner(&state, None, "skill-life".to_owned())
            .await
            .expect("archive");
        assert_eq!(archived.status, "archived");
        // Terminal: no second archive, no deprecation from archived.
        assert!(archive_skill_inner(&state, None, "skill-life".to_owned())
            .await
            .is_err());
        assert!(deprecate_skill_inner(&state, None, "skill-life".to_owned())
            .await
            .is_err());
        assert!(get_skill_inner(&state, "".to_owned()).await.is_err());
        assert!(get_skill_inner(&state, "missing".to_owned()).await.is_err());
    }

    #[tokio::test]
    async fn stats_versions_and_diff_round_trip() {
        let state = test_state().await;
        seed_skill(&state, "skill-stats").await;
        let stats = get_skill_stats_inner(&state, "skill-stats".to_owned())
            .await
            .expect("stats");
        assert_eq!(stats.usage_count, 0);
        assert!((stats.success_rate - 0.0).abs() < f64::EPSILON);
        assert!(!stats.needs_review);
        for _ in 0..4 {
            evidence_store::record_skill_use(&state.db, "skill-stats", false)
                .await
                .expect("use");
        }
        let stats = get_skill_stats_inner(&state, "skill-stats".to_owned())
            .await
            .expect("stats");
        assert_eq!(stats.usage_count, 4);
        assert_eq!(stats.failure_count, 4);
        assert!(stats.needs_review);
        // Version history + diff.
        evidence_store::create_skill_version(
            &state.db,
            "skill-stats",
            "1. Call `fs.read`. 2. Record the hash twice.",
            None,
        )
        .await
        .expect("version");
        let versions = list_skill_versions_inner(&state, "skill-stats".to_owned())
            .await
            .expect("versions");
        assert_eq!(versions.len(), 2);
        let lines = diff_skill_versions_inner(
            &state,
            "skill-stats".to_owned(),
            "skill-stats-v2".to_owned(),
        )
        .await
        .expect("diff");
        assert!(lines.iter().any(|line| line.kind == "added"));
        assert!(lines.iter().any(|line| line.kind == "removed"));
        assert!(
            diff_skill_versions_inner(&state, "skill-stats".to_owned(), "missing".to_owned())
                .await
                .is_err()
        );
        assert!(get_skill_stats_inner(&state, "missing".to_owned())
            .await
            .is_err());
    }

    #[tokio::test]
    async fn trigger_returns_immediately_and_evaluates_in_background() {
        let state = test_state().await;
        // Nothing due: returns at once without spawning work.
        let idle = trigger_skill_evaluation_inner(&state, None, Some(15))
            .await
            .expect("trigger");
        assert!(idle.started);
        assert!(!idle.due);
        assert!(trigger_skill_evaluation_inner(&state, None, Some(0))
            .await
            .is_err());
        // Seed 3 tool receipts, trigger with cadence 3: due, background pass
        // records the watermark (polled, bounded).
        for index in 0..3 {
            evidence_store::insert_evidence(
                &state.db,
                evidence_store::NewEvidence {
                    id: format!("gepa-ev-{index}"),
                    task_id: None,
                    agent_id: None,
                    session_id: Some("session-local".to_owned()),
                    kind: "tool_receipt".to_owned(),
                    label: None,
                    summary: None,
                    uri: None,
                    mime_type: None,
                    payload: serde_json::json!({"ok": true}),
                },
            )
            .await
            .expect("insert");
        }
        let fired = trigger_skill_evaluation_inner(&state, None, Some(3))
            .await
            .expect("trigger");
        assert!(fired.due);
        assert_eq!(fired.receipt_count, 3);
        let mut watermark = 0;
        for _ in 0..100 {
            watermark = skill_extractor::watermark(&state.db).await.expect("mark");
            if watermark >= 3 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        assert_eq!(watermark, 3);
    }
}
