//! GEPA (Generalized Experience-based Procedural Acquisition) evaluation
//! loop: every N tool calls, pause, review what worked, and close the
//! learning loop.
//!
//! The loop is deterministic and ledger-backed:
//! - A watermark (`gepa_evaluation` evidence rows carrying `receipt_count`)
//!   marks how many `tool_receipt` rows were reviewed so far.
//! - [`maybe_evaluate`] returns `None` until `every_n_calls` new receipts
//!   accumulate, so the hot path stays two cheap `COUNT` queries.
//! - [`evaluate`] reviews the window: recurring tool sequences across
//!   distinct tasks either reinforce an existing skill (usage counting) or
//!   propose a quarantined one; sustained-failing published skills
//!   auto-deprecate; stale published skills deprecate; everything is
//!   recorded as one `gepa_evaluation` evidence row (the next watermark).
//!
//! Evaluation never blocks task execution: desktop triggers run it on a
//! background task and return immediately.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use evidence_store::EvidenceDb;
use talos_common::{now_iso, Result};

use crate::extract::{ceiling_for, slug};

/// GEPA loop tuning.
#[derive(Debug, Clone)]
pub struct GepaConfig {
    /// Review every N new tool receipts (default 15, Hermes cadence).
    pub every_n_calls: i64,
    /// A tool sequence must recur across this many distinct tasks to count
    /// as a pattern (default 2).
    pub min_pattern_tasks: usize,
    /// Minimum successful calls in a task sequence to be pattern-eligible.
    pub min_sequence_len: usize,
    /// Days without use after which published skills go stale.
    pub stale_days: i64,
}

impl Default for GepaConfig {
    fn default() -> Self {
        Self {
            every_n_calls: 15,
            min_pattern_tasks: 2,
            min_sequence_len: 2,
            stale_days: evidence_store::SKILL_STALE_DAYS,
        }
    }
}

/// One skill application recorded during evaluation.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SkillUse {
    /// Skill id.
    pub skill_id: String,
    /// Task the pattern was observed on.
    pub task_id: String,
    /// Whether that task completed verified.
    pub success: bool,
}

/// Outcome of one GEPA evaluation pass.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EvaluationReport {
    /// RFC 3339 evaluation time.
    pub evaluated_at: String,
    /// Total `tool_receipt` rows reviewed through (new watermark).
    pub receipt_count: i64,
    /// Receipts reviewed in this window.
    pub window_size: i64,
    /// Distinct recurring patterns found.
    pub patterns_found: usize,
    /// New quarantined skill ids proposed.
    pub skills_proposed: Vec<String>,
    /// Skill applications recorded.
    pub skills_used: Vec<SkillUse>,
    /// Published skill ids auto-deprecated for sustained failure.
    pub skills_deprecated: Vec<String>,
    /// Stale published skill ids deprecated.
    pub skills_staled: Vec<String>,
    /// Skill ids below the review threshold (human look needed).
    pub needs_review: Vec<String>,
    /// Evidence id of the recorded `gepa_evaluation` row.
    pub evidence_id: String,
}

/// Read the current watermark: `receipt_count` of the newest
/// `gepa_evaluation` row (0 when the loop never ran).
pub async fn watermark(db: &EvidenceDb) -> Result<i64> {
    let latest = evidence_store::latest_evidence_by_kind(db, "gepa_evaluation").await?;
    let Some(row) = latest else {
        return Ok(0);
    };
    let count = row
        .payload_json
        .as_deref()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
        .and_then(|payload| payload.get("receipt_count")?.as_i64())
        .unwrap_or(0);
    Ok(count.max(0))
}

/// True when at least `config.every_n_calls` new tool receipts accumulated
/// since the watermark (cheap: two count queries, no window scan).
pub async fn evaluation_due(db: &EvidenceDb, config: &GepaConfig) -> Result<bool> {
    let total = evidence_store::count_evidence_by_kind(db, "tool_receipt").await?;
    let marked = watermark(db).await?;
    Ok(total - marked >= config.every_n_calls.max(1))
}

/// Run the loop when due, else return `None` without scanning. The caller
/// decides whether to block on this or spawn it in the background.
pub async fn maybe_evaluate(
    db: &EvidenceDb,
    config: &GepaConfig,
) -> Result<Option<EvaluationReport>> {
    if !evaluation_due(db, config).await? {
        return Ok(None);
    }
    Ok(Some(evaluate(db, config).await?))
}

/// Task outcomes keyed by task id (completed + verified = success).
async fn task_outcomes(db: &EvidenceDb, task_ids: &BTreeSet<String>) -> HashMap<String, bool> {
    let mut outcomes = HashMap::new();
    for task_id in task_ids {
        let success = match evidence_store::get_task(db, task_id).await {
            Ok(Some(task)) => task.status == "completed" && task.fidelity == "verified",
            _ => false,
        };
        outcomes.insert(task_id.clone(), success);
    }
    outcomes
}

/// Evaluate one window: detect patterns, reinforce or propose skills,
/// deprecate failures, record the watermark. Deterministic for a fixed
/// ledger (sorted patterns, sorted ids).
pub async fn evaluate(db: &EvidenceDb, config: &GepaConfig) -> Result<EvaluationReport> {
    let started = std::time::Instant::now();
    let total = evidence_store::count_evidence_by_kind(db, "tool_receipt").await?;
    let marked = watermark(db).await?;
    let window_size = (total - marked).max(0);

    // Window: newest (total - marked) tool receipts, oldest first.
    let rows = evidence_store::list_recent_evidence(db, 1000).await?;
    let mut window: Vec<(String, String, String, String)> = Vec::new();
    for row in rows.iter().filter(|row| row.kind == "tool_receipt") {
        let Some(payload) = row.payload_json.as_deref() else {
            continue;
        };
        let receipt: tool_runtime::ToolReceipt = match serde_json::from_str(payload) {
            Ok(receipt) => receipt,
            Err(_) => continue,
        };
        if receipt.status != tool_runtime::ToolStatus::Success {
            continue;
        }
        let Some(task_id) = row.task_id.clone() else {
            continue;
        };
        window.push((receipt.executed_at, receipt.id, task_id, receipt.tool_id));
    }
    window.sort();
    if window.len() as i64 > window_size {
        let skip = window.len() - window_size as usize;
        window.drain(..skip);
    }

    // Per-task sequences (successful calls, execution order).
    let mut sequences: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (_, _, task_id, tool_id) in &window {
        sequences
            .entry(task_id.clone())
            .or_default()
            .push(tool_id.clone());
    }
    sequences.retain(|_, tools| tools.len() >= config.min_sequence_len.max(1));

    // Recurring patterns: identical sequences across distinct tasks.
    let mut pattern_tasks: BTreeMap<Vec<String>, BTreeSet<String>> = BTreeMap::new();
    for (task_id, tools) in &sequences {
        pattern_tasks
            .entry(tools.clone())
            .or_default()
            .insert(task_id.clone());
    }
    let patterns: Vec<(Vec<String>, BTreeSet<String>)> = pattern_tasks
        .into_iter()
        .filter(|(_, tasks)| tasks.len() >= config.min_pattern_tasks.max(2))
        .collect();

    let outcomes = task_outcomes(
        db,
        &patterns
            .iter()
            .flat_map(|(_, tasks)| tasks.iter().cloned())
            .collect(),
    )
    .await;

    let skills = evidence_store::list_skills(db, None, 500).await?;
    let mut report = EvaluationReport {
        evaluated_at: now_iso(),
        receipt_count: total,
        window_size,
        patterns_found: patterns.len(),
        skills_proposed: Vec::new(),
        skills_used: Vec::new(),
        skills_deprecated: Vec::new(),
        skills_staled: Vec::new(),
        needs_review: Vec::new(),
        evidence_id: String::new(),
    };

    for (tools, tasks) in &patterns {
        let mut tool_set: Vec<String> = tools.clone();
        tool_set.sort();
        tool_set.dedup();
        // An existing skill covers this pattern when its required-tool set
        // matches exactly (both sorted).
        let covered = skills.iter().find(|skill| {
            let mut required: Vec<String> =
                serde_json::from_str(&skill.required_tools).unwrap_or_default();
            required.sort();
            required == tool_set
        });
        match covered {
            Some(skill) => {
                for task_id in tasks {
                    let success = outcomes.get(task_id).copied().unwrap_or(false);
                    let updated = evidence_store::record_skill_use(db, &skill.id, success).await?;
                    let _ = updated;
                    report.skills_used.push(SkillUse {
                        skill_id: skill.id.clone(),
                        task_id: task_id.clone(),
                        success,
                    });
                }
            }
            None => {
                let name = format!("pattern-{}-skill", slug(&tools.join("-")));
                let id = format!("skill-{}", slug(&tools.join("-")));
                if evidence_store::get_skill(db, &id).await?.is_none() {
                    let steps = tools
                        .iter()
                        .enumerate()
                        .map(|(index, tool)| format!("{}. Call `{tool}`.", index + 1))
                        .collect::<Vec<_>>()
                        .join("\n");
                    let created = evidence_store::create_skill(
                        db,
                        evidence_store::NewSkill {
                            id: id.clone(),
                            name: name.chars().take(128).collect(),
                            description: format!(
                                "Recurring tool pattern observed on {} tasks: {}.",
                                tasks.len(),
                                tools.join(", ")
                            )
                            .chars()
                            .take(300)
                            .collect(),
                            procedure_text: steps,
                            source_task_id: None,
                            version: "1".to_owned(),
                            test_fixture_ref: None,
                            required_tools: tool_set.clone(),
                            permission_ceiling: ceiling_for(&tool_set),
                        },
                    )
                    .await?;
                    let text = memory_indexer::skill_text(
                        &created.name,
                        &created.description,
                        &created.procedure_text,
                    );
                    let _ = vector_store::upsert_entry(
                        db,
                        vector_store::NewVectorEntry {
                            source_type: "skill".to_owned(),
                            source_id: created.id.clone(),
                            task_id: None,
                            text,
                            verified: false,
                            metadata: serde_json::json!({ "status": created.status }),
                        },
                    )
                    .await;
                    report.skills_proposed.push(created.id);
                }
            }
        }
    }

    // Sustained failure → auto-deprecate published skills.
    let refreshed = evidence_store::list_skills(db, None, 500).await?;
    for skill in &refreshed {
        if evidence_store::should_deprecate(skill) {
            evidence_store::update_skill_status(db, &skill.id, "deprecated").await?;
            report.skills_deprecated.push(skill.id.clone());
        }
    }
    // Staleness sweep.
    let staled =
        evidence_store::deprecate_stale_skills(db, &report.evaluated_at, config.stale_days).await?;
    report.skills_staled = staled;
    // Review flags (below threshold with enough uses, still live).
    let flagged = evidence_store::list_skills(db, None, 500).await?;
    for skill in flagged {
        if skill.status != "deprecated"
            && skill.status != "archived"
            && evidence_store::needs_review(&skill)
        {
            report.needs_review.push(skill.id.clone());
        }
    }
    report.needs_review.sort();
    report.needs_review.dedup();
    report
        .skills_used
        .sort_by(|a, b| (&a.skill_id, &a.task_id).cmp(&(&b.skill_id, &b.task_id)));

    let evidence_id = format!("gepa-eval-{}", report.evaluated_at.replace([':', '.'], "-"));
    let payload = serde_json::json!({
        "receipt_count": total,
        "window_size": window_size,
        "patterns_found": report.patterns_found,
        "skills_proposed": report.skills_proposed,
        "skills_used": report.skills_used,
        "skills_deprecated": report.skills_deprecated,
        "skills_staled": report.skills_staled,
        "needs_review": report.needs_review,
    });
    let row = evidence_store::insert_evidence(
        db,
        evidence_store::NewEvidence {
            id: evidence_id,
            task_id: None,
            agent_id: None,
            session_id: Some("session-local".to_owned()),
            kind: "gepa_evaluation".to_owned(),
            label: Some(format!("GEPA pass over {window_size} receipts")),
            summary: Some(format!(
                "{} patterns, {} proposed, {} uses recorded",
                report.patterns_found,
                report.skills_proposed.len(),
                report.skills_used.len()
            )),
            uri: None,
            mime_type: None,
            payload,
        },
    )
    .await?;
    evidence_store::transition_evidence_status(db, &row.id, "verified").await?;
    report.evidence_id = row.id;
    let duration_ms = started.elapsed().as_millis().clamp(0, i64::MAX as u128) as i64;
    talos_telemetry::record_now(
        "skill_extractor",
        "gepa_evaluate",
        None,
        None,
        talos_telemetry::SpanStatus::Ok,
        &format!("{} patterns", report.patterns_found),
        duration_ms,
        &[
            ("gepa.patterns", serde_json::json!(report.patterns_found)),
            ("gepa.window", serde_json::json!(window_size)),
        ],
    );
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn db() -> EvidenceDb {
        EvidenceDb::connect_memory().await.expect("memory db")
    }

    async fn seed_task(db: &EvidenceDb, id: &str) {
        evidence_store::enqueue_task(
            db,
            evidence_store::EnqueueParams {
                id,
                title: "seed",
                description: None,
                priority: "normal",
                max_attempts: 3,
                timeout_ms: 5000,
                backoff_ms: 500,
                scenario: "quick_success",
                parent_task_id: None,
            },
        )
        .await
        .expect("enqueue");
    }

    fn receipt(id: &str, task: &str, tool: &str, at: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "toolId": tool,
            "taskId": task,
            "dryRun": false,
            "status": "success",
            "output": {},
            "error": null,
            "policyDecision": "allow",
            "policyReason": "allowed",
            "evidenceId": null,
            "compensationPlanId": null,
            "executionFidelity": "live",
            "executedAt": at,
        })
    }

    async fn seed_receipt(db: &EvidenceDb, id: &str, task: &str, tool: &str, at: &str) {
        evidence_store::insert_evidence(
            db,
            evidence_store::NewEvidence {
                id: id.to_owned(),
                task_id: Some(task.to_owned()),
                agent_id: None,
                session_id: Some("session-local".to_owned()),
                kind: "tool_receipt".to_owned(),
                label: None,
                summary: None,
                uri: None,
                mime_type: None,
                payload: receipt(id, task, tool, at),
            },
        )
        .await
        .expect("insert");
    }

    fn config(n: i64) -> GepaConfig {
        GepaConfig {
            every_n_calls: n,
            min_pattern_tasks: 2,
            min_sequence_len: 2,
            stale_days: 30,
        }
    }

    #[tokio::test]
    async fn evaluation_runs_every_n_calls() {
        let db = db().await;
        // Below the cadence: not due, no scan.
        assert!(!evaluation_due(&db, &config(15)).await.expect("due"));
        assert!(maybe_evaluate(&db, &config(15))
            .await
            .expect("maybe")
            .is_none());
        // 15 receipts across tasks (no recurring pattern): due, runs, no proposals.
        for index in 0..15 {
            let task = format!("task-{index}");
            seed_task(&db, &task).await;
            seed_receipt(
                &db,
                &format!("ev-{index}"),
                &task,
                &format!("tool.{index}"),
                &format!("2026-01-01T00:00:{index:02}Z"),
            )
            .await;
        }
        assert!(evaluation_due(&db, &config(15)).await.expect("due"));
        let report = maybe_evaluate(&db, &config(15))
            .await
            .expect("maybe")
            .expect("report");
        assert_eq!(report.window_size, 15);
        assert_eq!(report.receipt_count, 15);
        assert!(!report.evidence_id.is_empty());
        // Watermark advanced: not due again until 15 more.
        assert!(!evaluation_due(&db, &config(15)).await.expect("due"));
        assert_eq!(watermark(&db).await.expect("mark"), 15);
    }

    #[tokio::test]
    async fn recurring_pattern_proposes_one_skill() {
        let db = db().await;
        for task in ["task-a", "task-b"] {
            seed_task(&db, task).await;
            seed_receipt(
                &db,
                &format!("{task}-1"),
                task,
                "fs.read",
                "2026-01-01T00:00:01Z",
            )
            .await;
            seed_receipt(
                &db,
                &format!("{task}-2"),
                task,
                "fs.list",
                "2026-01-01T00:00:02Z",
            )
            .await;
        }
        // Single-call task: below min_sequence_len, ignored.
        seed_task(&db, "task-c").await;
        seed_receipt(&db, "c-1", "task-c", "fs.read", "2026-01-01T00:00:03Z").await;
        let report = evaluate(&db, &config(1)).await.expect("evaluate");
        assert_eq!(report.patterns_found, 1);
        assert_eq!(report.skills_proposed.len(), 1);
        let skill: evidence_store::SkillRow =
            evidence_store::get_skill(&db, &report.skills_proposed[0])
                .await
                .expect("get")
                .expect("row");
        assert_eq!(skill.status, "quarantined");
        let tools: Vec<String> = serde_json::from_str(&skill.required_tools).expect("tools");
        assert_eq!(tools, vec!["fs.list".to_owned(), "fs.read".to_owned()]);
        // Deterministic: rerun over new tasks with the same pattern records
        // uses against the covering skill instead of proposing a duplicate.
        for task in ["task-d", "task-e"] {
            seed_task(&db, task).await;
            seed_receipt(
                &db,
                &format!("{task}-1"),
                task,
                "fs.read",
                "2026-01-01T00:01:01Z",
            )
            .await;
            seed_receipt(
                &db,
                &format!("{task}-2"),
                task,
                "fs.list",
                "2026-01-01T00:01:02Z",
            )
            .await;
        }
        let second = evaluate(&db, &config(1)).await.expect("rerun");
        assert!(second.skills_proposed.is_empty());
        assert_eq!(second.skills_used.len(), 2);
    }

    #[tokio::test]
    async fn matching_skill_records_usage() {
        let db = db().await;
        // Existing quarantined skill covering the pattern: uses recorded,
        // including failure for the non-verified task.
        evidence_store::create_skill(
            &db,
            evidence_store::NewSkill {
                id: "skill-fs-pair".to_owned(),
                name: "fs pair".to_owned(),
                description: "Read then list.".to_owned(),
                procedure_text: "1. Call fs.read. 2. Call fs.list.".to_owned(),
                source_task_id: None,
                version: "1".to_owned(),
                test_fixture_ref: None,
                required_tools: vec!["fs.read".to_owned(), "fs.list".to_owned()],
                permission_ceiling: "low".to_owned(),
            },
        )
        .await
        .expect("create");
        for task in ["task-a", "task-b"] {
            seed_task(&db, task).await;
            seed_receipt(
                &db,
                &format!("{task}-1"),
                task,
                "fs.read",
                "2026-01-01T00:00:01Z",
            )
            .await;
            seed_receipt(
                &db,
                &format!("{task}-2"),
                task,
                "fs.list",
                "2026-01-01T00:00:02Z",
            )
            .await;
        }
        let report = evaluate(&db, &config(1)).await.expect("evaluate");
        assert!(report.skills_proposed.is_empty());
        assert_eq!(report.skills_used.len(), 2);
        let skill = evidence_store::get_skill(&db, "skill-fs-pair")
            .await
            .expect("get")
            .expect("row");
        assert_eq!(skill.usage_count, 2);
        // Seeded tasks are queued (not completed verified) → failures.
        assert_eq!(skill.failure_count, 2);
    }
}
