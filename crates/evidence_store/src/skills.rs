//! Skill registry: procedural knowledge distilled from successful tasks.
//!
//! Skills are project-level capabilities (`procedure_text`, required tools,
//! permission ceiling, test fixture). New skills are born `quarantined`;
//! promotion to `validated` / `published` is an explicit transition, never
//! implicit. The memory indexer embeds `procedure_text` for semantic recall.

use talos_common::{now_iso, Result, TalosError};

use crate::EvidenceDb;

/// Lifecycle status for a skill.
///
/// `quarantined` → `validated` → `published` is the promotion path;
/// `published` → `deprecated` marks stale or failing skills; any live
/// status → `archived` retires a skill (terminal).
pub const SKILL_STATUSES: &[&str] = &[
    "quarantined",
    "validated",
    "published",
    "deprecated",
    "archived",
];

/// Success-rate threshold below which a used skill needs review (GEPA).
pub const SKILL_REVIEW_THRESHOLD: f64 = 0.70;

/// Minimum uses before the review threshold applies.
pub const SKILL_REVIEW_MIN_USES: i64 = 3;

/// Uses at or above this count with a sub-threshold rate auto-deprecate a
/// published skill during GEPA evaluation (sustained failure, not noise).
pub const SKILL_DEPRECATE_MIN_USES: i64 = 5;

/// Days without use after which a published skill counts as stale.
pub const SKILL_STALE_DAYS: i64 = 30;

fn valid_id(what: &'static str, value: &str) -> Result<()> {
    if value.is_empty() || value.len() > 128 {
        return Err(TalosError::InvalidId {
            what,
            reason: "must be 1..=128 chars".to_owned(),
        });
    }
    Ok(())
}

/// Full `skills` row.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct SkillRow {
    /// Skill identifier.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// What the skill does.
    pub description: String,
    /// Step-by-step procedure text (embedded for semantic recall).
    pub procedure_text: String,
    /// Task the skill was distilled from, if any.
    pub source_task_id: Option<String>,
    /// Skill version.
    pub version: String,
    /// Test fixture reference proving the skill works.
    pub test_fixture_ref: Option<String>,
    /// JSON array of required tool ids.
    pub required_tools: String,
    /// Permission ceiling (`low` | `medium` | `high` | `irreversible`).
    pub permission_ceiling: String,
    /// Lifecycle status.
    pub status: String,
    /// RFC 3339 creation timestamp.
    pub created_at: String,
    /// Times this skill matched a task pattern (GEPA usage tracking).
    pub usage_count: i64,
    /// Uses on tasks that completed verified.
    pub success_count: i64,
    /// Uses on tasks that did not complete verified.
    pub failure_count: i64,
    /// `success_count / usage_count` (0.0 when unused).
    pub confidence_score: f64,
    /// RFC 3339 timestamp of the most recent recorded use, if any.
    pub last_used_at: Option<String>,
    /// Parent skill id for improved versions (`None` for originals).
    pub parent_version: Option<String>,
}

/// Payload for [`create_skill`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSkill {
    /// Skill identifier.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// What the skill does.
    pub description: String,
    /// Step-by-step procedure text.
    pub procedure_text: String,
    /// Task the skill was distilled from, if any.
    pub source_task_id: Option<String>,
    /// Skill version.
    pub version: String,
    /// Test fixture reference.
    pub test_fixture_ref: Option<String>,
    /// Required tool ids.
    pub required_tools: Vec<String>,
    /// Permission ceiling.
    pub permission_ceiling: String,
}

/// Create a skill in `quarantined` status (promotion is explicit).
pub async fn create_skill(db: &EvidenceDb, skill: NewSkill) -> Result<SkillRow> {
    valid_id("skill_id", &skill.id)?;
    if skill.name.trim().is_empty() || skill.name.len() > 256 {
        return Err(TalosError::InvalidId {
            what: "skill_name",
            reason: "must be 1..=256 chars".to_owned(),
        });
    }
    if skill.procedure_text.trim().is_empty() {
        return Err(TalosError::InvalidId {
            what: "procedure_text",
            reason: "must not be empty".to_owned(),
        });
    }
    if !["low", "medium", "high", "irreversible"].contains(&skill.permission_ceiling.as_str()) {
        return Err(TalosError::InvalidId {
            what: "permission_ceiling",
            reason: "must be low|medium|high|irreversible".to_owned(),
        });
    }
    if let Some(task_id) = skill.source_task_id.as_deref() {
        valid_id("source_task_id", task_id)?;
        let exists: Option<String> =
            sqlx::query_scalar("SELECT id FROM runtime_tasks WHERE id = ?")
                .bind(task_id)
                .fetch_optional(db.pool())
                .await
                .map_err(|err| TalosError::Storage(err.to_string()))?;
        if exists.is_none() {
            return Err(TalosError::InvalidId {
                what: "source_task_id",
                reason: "task does not exist".to_owned(),
            });
        }
    }
    let required_tools = serde_json::to_string(&skill.required_tools).map_err(TalosError::from)?;
    let created_at = now_iso();
    sqlx::query(
        "INSERT INTO skills
         (id, name, description, procedure_text, source_task_id, version,
          test_fixture_ref, required_tools, permission_ceiling, status, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'quarantined', ?)",
    )
    .bind(&skill.id)
    .bind(&skill.name)
    .bind(&skill.description)
    .bind(&skill.procedure_text)
    .bind(&skill.source_task_id)
    .bind(&skill.version)
    .bind(&skill.test_fixture_ref)
    .bind(&required_tools)
    .bind(&skill.permission_ceiling)
    .bind(&created_at)
    .execute(db.pool())
    .await
    .map_err(|err| TalosError::Storage(err.to_string()))?;
    Ok(SkillRow {
        id: skill.id,
        name: skill.name,
        description: skill.description,
        procedure_text: skill.procedure_text,
        source_task_id: skill.source_task_id,
        version: skill.version,
        test_fixture_ref: skill.test_fixture_ref,
        required_tools,
        permission_ceiling: skill.permission_ceiling,
        status: "quarantined".to_owned(),
        created_at,
        usage_count: 0,
        success_count: 0,
        failure_count: 0,
        confidence_score: 0.0,
        last_used_at: None,
        parent_version: None,
    })
}

/// Fetch one skill by id.
pub async fn get_skill(db: &EvidenceDb, id: &str) -> Result<Option<SkillRow>> {
    valid_id("skill_id", id)?;
    sqlx::query_as::<_, SkillRow>("SELECT * FROM skills WHERE id = ?")
        .bind(id)
        .fetch_optional(db.pool())
        .await
        .map_err(|err| TalosError::Storage(err.to_string()))
}

/// Newest skill distilled from one task, if any (GEPA emission checks).
pub async fn latest_skill_for_task(db: &EvidenceDb, task_id: &str) -> Result<Option<SkillRow>> {
    valid_id("source_task_id", task_id)?;
    sqlx::query_as::<_, SkillRow>(
        "SELECT * FROM skills WHERE source_task_id = ? ORDER BY rowid DESC LIMIT 1",
    )
    .bind(task_id)
    .fetch_optional(db.pool())
    .await
    .map_err(|err| TalosError::Storage(err.to_string()))
}

/// List skills, newest last by rowid, bounded (optional status filter).
pub async fn list_skills(
    db: &EvidenceDb,
    status: Option<&str>,
    limit: i64,
) -> Result<Vec<SkillRow>> {
    let limit = limit.clamp(1, 500);
    if let Some(wanted) = status {
        if !SKILL_STATUSES.contains(&wanted) {
            return Err(TalosError::InvalidId {
                what: "status",
                reason: "must be quarantined|validated|published".to_owned(),
            });
        }
        sqlx::query_as::<_, SkillRow>(
            "SELECT * FROM skills WHERE status = ? ORDER BY rowid ASC LIMIT ?",
        )
        .bind(wanted)
        .bind(limit)
        .fetch_all(db.pool())
        .await
        .map_err(|err| TalosError::Storage(err.to_string()))
    } else {
        sqlx::query_as::<_, SkillRow>("SELECT * FROM skills ORDER BY rowid ASC LIMIT ?")
            .bind(limit)
            .fetch_all(db.pool())
            .await
            .map_err(|err| TalosError::Storage(err.to_string()))
    }
}

/// Explicit lifecycle moves: `quarantined` → `validated` → `published`
/// (no skips), `published` → `deprecated` (stale or failing), and any live
/// status → `archived` (terminal; archiving an archived skill is rejected).
pub async fn update_skill_status(db: &EvidenceDb, id: &str, to: &str) -> Result<SkillRow> {
    valid_id("skill_id", id)?;
    let row = get_skill(db, id)
        .await?
        .ok_or_else(|| TalosError::InvalidId {
            what: "skill_id",
            reason: "skill does not exist".to_owned(),
        })?;
    let allowed = matches!(
        (row.status.as_str(), to),
        ("quarantined", "validated") | ("validated", "published") | ("published", "deprecated")
    ) || (to == "archived" && row.status != "archived");
    if !allowed {
        return Err(TalosError::InvalidId {
            what: "status",
            reason:
                "allowed: quarantined->validated->published, published->deprecated, any->archived"
                    .to_owned(),
        });
    }
    sqlx::query("UPDATE skills SET status = ? WHERE id = ?")
        .bind(to)
        .bind(id)
        .execute(db.pool())
        .await
        .map_err(|err| TalosError::Storage(err.to_string()))?;
    get_skill(db, id)
        .await?
        .ok_or_else(|| TalosError::Storage("skill missing after update".to_owned()))
}

/// Success rate in [0, 1] (0.0 when never used).
#[must_use]
pub fn success_rate(row: &SkillRow) -> f64 {
    if row.usage_count <= 0 {
        return 0.0;
    }
    (f64::from(row.success_count as u32) / f64::from(row.usage_count as u32)).clamp(0.0, 1.0)
}

/// True when a skill has enough uses below the review threshold to need a
/// human look (GEPA review flag; not an automatic transition).
#[must_use]
pub fn needs_review(row: &SkillRow) -> bool {
    row.usage_count >= SKILL_REVIEW_MIN_USES && success_rate(row) < SKILL_REVIEW_THRESHOLD
}

/// True when sustained failure should auto-deprecate a published skill.
#[must_use]
pub fn should_deprecate(row: &SkillRow) -> bool {
    row.status == "published"
        && row.usage_count >= SKILL_DEPRECATE_MIN_USES
        && success_rate(row) < SKILL_REVIEW_THRESHOLD
}

/// Record one skill application outcome: increments usage, success/failure,
/// stamps `last_used_at`, and recomputes `confidence_score` as the running
/// success rate (5+ uses at 100% success therefore promote to 1.0).
pub async fn record_skill_use(db: &EvidenceDb, id: &str, success: bool) -> Result<SkillRow> {
    valid_id("skill_id", id)?;
    let row = get_skill(db, id)
        .await?
        .ok_or_else(|| TalosError::InvalidId {
            what: "skill_id",
            reason: "skill does not exist".to_owned(),
        })?;
    let usage_count = row.usage_count.saturating_add(1);
    let success_count = row.success_count.saturating_add(i64::from(success));
    let failure_count = row.failure_count.saturating_add(i64::from(!success));
    let confidence_score =
        (f64::from(success_count as u32) / f64::from(usage_count as u32)).clamp(0.0, 1.0);
    let last_used_at = now_iso();
    sqlx::query(
        "UPDATE skills SET usage_count = ?, success_count = ?, failure_count = ?,
         confidence_score = ?, last_used_at = ? WHERE id = ?",
    )
    .bind(usage_count)
    .bind(success_count)
    .bind(failure_count)
    .bind(confidence_score)
    .bind(&last_used_at)
    .bind(id)
    .execute(db.pool())
    .await
    .map_err(|err| TalosError::Storage(err.to_string()))?;
    get_skill(db, id)
        .await?
        .ok_or_else(|| TalosError::Storage("skill missing after update".to_owned()))
}

/// Bump a version string deterministically: trailing numeric segment +1
/// (`"1"` → `"2"`, `"0.1.0"` → `"0.1.1"`); non-numeric versions append `.1`.
#[must_use]
pub fn next_version(version: &str) -> String {
    let mut parts: Vec<String> = version.split('.').map(str::to_owned).collect();
    if let Some(last) = parts.last() {
        if let Ok(number) = last.parse::<u64>() {
            parts.pop();
            parts.push((number.saturating_add(1)).to_string());
            return parts.join(".");
        }
    }
    if parts.is_empty() {
        return "1".to_owned();
    }
    format!("{version}.1")
}

/// Create an improved version of a skill: same identity lineage under a new
/// `{id}-v{version}` row, `parent_version` pointing at the improved row, body
/// replaced, counters reset, born `quarantined` (re-validation required).
/// The old row is untouched, so rollback is a status read away.
pub async fn create_skill_version(
    db: &EvidenceDb,
    id: &str,
    procedure_text: &str,
    description: Option<&str>,
) -> Result<SkillRow> {
    valid_id("skill_id", id)?;
    if procedure_text.trim().is_empty() || procedure_text.len() > 32768 {
        return Err(TalosError::InvalidId {
            what: "procedure_text",
            reason: "must be 1..=32768 chars".to_owned(),
        });
    }
    let parent = get_skill(db, id)
        .await?
        .ok_or_else(|| TalosError::InvalidId {
            what: "skill_id",
            reason: "skill does not exist".to_owned(),
        })?;
    let version = next_version(&parent.version);
    let child_id = format!("{}-v{}", parent.id, version);
    if get_skill(db, &child_id).await?.is_some() {
        return Err(TalosError::InvalidId {
            what: "skill_id",
            reason: "that version already exists".to_owned(),
        });
    }
    let created_at = now_iso();
    sqlx::query(
        "INSERT INTO skills
         (id, name, description, procedure_text, source_task_id, version,
          test_fixture_ref, required_tools, permission_ceiling, status, created_at,
          usage_count, success_count, failure_count, confidence_score, last_used_at,
          parent_version)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'quarantined', ?, 0, 0, 0, 0.0, NULL, ?)",
    )
    .bind(&child_id)
    .bind(&parent.name)
    .bind(description.unwrap_or(&parent.description))
    .bind(procedure_text)
    .bind(&parent.source_task_id)
    .bind(&version)
    .bind(&parent.test_fixture_ref)
    .bind(&parent.required_tools)
    .bind(&parent.permission_ceiling)
    .bind(&created_at)
    .bind(&parent.id)
    .execute(db.pool())
    .await
    .map_err(|err| TalosError::Storage(err.to_string()))?;
    get_skill(db, &child_id)
        .await?
        .ok_or_else(|| TalosError::Storage("skill missing after insert".to_owned()))
}

/// List a skill plus every improved version (parent chain), oldest first.
pub async fn list_skill_versions(db: &EvidenceDb, id: &str) -> Result<Vec<SkillRow>> {
    valid_id("skill_id", id)?;
    sqlx::query_as::<_, SkillRow>(
        "SELECT * FROM skills WHERE id = ? OR parent_version = ? ORDER BY rowid ASC",
    )
    .bind(id)
    .bind(id)
    .fetch_all(db.pool())
    .await
    .map_err(|err| TalosError::Storage(err.to_string()))
}

/// One line of a procedure diff.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SkillDiffLine {
    /// `same` | `removed` | `added`.
    pub kind: String,
    /// Line text.
    pub text: String,
}

/// Deterministic line diff between two procedure texts (LCS over lines,
/// bounded to 2000 lines per side; longer inputs are truncated with a
/// marker line so output stays bounded).
#[must_use]
pub fn skill_diff(old_text: &str, new_text: &str) -> Vec<SkillDiffLine> {
    const CAP: usize = 2000;
    let old_lines: Vec<&str> = old_text.lines().take(CAP).collect();
    let new_lines: Vec<&str> = new_text.lines().take(CAP).collect();
    let old_truncated = old_text.lines().count() > CAP;
    let new_truncated = new_text.lines().count() > CAP;
    // LCS table (lengths only, rebuilt row by row for memory).
    let (n, m) = (old_lines.len(), new_lines.len());
    let mut prev = vec![0usize; m + 1];
    let mut table: Vec<Vec<usize>> = Vec::with_capacity(n + 1);
    table.push(prev.clone());
    for old_line in &old_lines {
        let mut current = vec![0usize; m + 1];
        for (j, new_line) in new_lines.iter().enumerate() {
            current[j + 1] = if old_line == new_line {
                prev[j] + 1
            } else {
                prev[j + 1].max(current[j])
            };
        }
        prev.clone_from(&current);
        table.push(current);
    }
    // Backtrack from (n, m).
    let mut out = Vec::new();
    let (mut i, mut j) = (n, m);
    while i > 0 && j > 0 {
        if old_lines[i - 1] == new_lines[j - 1] {
            out.push(SkillDiffLine {
                kind: "same".to_owned(),
                text: old_lines[i - 1].to_owned(),
            });
            i -= 1;
            j -= 1;
        } else if table[i - 1][j] >= table[i][j - 1] {
            out.push(SkillDiffLine {
                kind: "removed".to_owned(),
                text: old_lines[i - 1].to_owned(),
            });
            i -= 1;
        } else {
            out.push(SkillDiffLine {
                kind: "added".to_owned(),
                text: new_lines[j - 1].to_owned(),
            });
            j -= 1;
        }
    }
    while i > 0 {
        out.push(SkillDiffLine {
            kind: "removed".to_owned(),
            text: old_lines[i - 1].to_owned(),
        });
        i -= 1;
    }
    while j > 0 {
        out.push(SkillDiffLine {
            kind: "added".to_owned(),
            text: new_lines[j - 1].to_owned(),
        });
        j -= 1;
    }
    out.reverse();
    if old_truncated || new_truncated {
        out.push(SkillDiffLine {
            kind: "same".to_owned(),
            text: "[…truncated at 2000 lines per side…]".to_owned(),
        });
    }
    out
}

/// Deprecate published skills gone stale: never used and created more than
/// `stale_days` ago, or last used more than `stale_days` ago. `now_iso`
/// is a parameter so tests are deterministic. Returns deprecated ids.
pub async fn deprecate_stale_skills(
    db: &EvidenceDb,
    now_iso: &str,
    stale_days: i64,
) -> Result<Vec<String>> {
    let stale_days = stale_days.clamp(1, 3650);
    let rows = list_skills(db, Some("published"), 500).await?;
    let mut deprecated = Vec::new();
    for row in rows {
        let reference = row.last_used_at.as_deref().unwrap_or(&row.created_at);
        if days_between(reference, now_iso) >= stale_days {
            update_skill_status(db, &row.id, "deprecated").await?;
            deprecated.push(row.id);
        }
    }
    deprecated.sort();
    Ok(deprecated)
}

/// Whole days from `from_iso` to `to_iso` (unparseable inputs count as 0,
/// i.e. never stale — fail closed toward keeping the skill).
fn days_between(from_iso: &str, to_iso: &str) -> i64 {
    let parse = |raw: &str| {
        chrono::DateTime::parse_from_rfc3339(raw)
            .map(|fixed| fixed.with_timezone(&chrono::Utc))
            .ok()
    };
    match (parse(from_iso), parse(to_iso)) {
        (Some(from), Some(to)) => (to - from).num_days().max(0),
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn db() -> EvidenceDb {
        EvidenceDb::connect_memory().await.expect("memory db")
    }

    fn skill(id: &str) -> NewSkill {
        NewSkill {
            id: id.to_owned(),
            name: "read then hash".to_owned(),
            description: "Read a file and hash it.".to_owned(),
            procedure_text: "1. fs.read the path. 2. Record the sha256 hash.".to_owned(),
            source_task_id: None,
            version: "0.1.0".to_owned(),
            test_fixture_ref: None,
            required_tools: vec!["fs.read".to_owned()],
            permission_ceiling: "low".to_owned(),
        }
    }

    #[tokio::test]
    async fn creates_quarantined_by_default() {
        let db = db().await;
        let row = create_skill(&db, skill("skill-1")).await.expect("create");
        assert_eq!(row.status, "quarantined");
        let fetched = get_skill(&db, "skill-1").await.expect("get").expect("row");
        assert_eq!(fetched.procedure_text, row.procedure_text);
    }

    #[tokio::test]
    async fn promotion_is_explicit_and_ordered() {
        let db = db().await;
        create_skill(&db, skill("skill-2")).await.expect("create");
        // No skips, no demotion.
        assert!(update_skill_status(&db, "skill-2", "published")
            .await
            .is_err());
        let validated = update_skill_status(&db, "skill-2", "validated")
            .await
            .expect("promote");
        assert_eq!(validated.status, "validated");
        let published = update_skill_status(&db, "skill-2", "published")
            .await
            .expect("publish");
        assert_eq!(published.status, "published");
        assert!(update_skill_status(&db, "skill-2", "quarantined")
            .await
            .is_err());
        // Deprecation only from published; archiving from any live status.
        let deprecated = update_skill_status(&db, "skill-2", "deprecated")
            .await
            .expect("deprecate");
        assert_eq!(deprecated.status, "deprecated");
        let archived = update_skill_status(&db, "skill-2", "archived")
            .await
            .expect("archive");
        assert_eq!(archived.status, "archived");
        assert!(update_skill_status(&db, "skill-2", "archived")
            .await
            .is_err());
        // Unknown status values are rejected.
        assert!(update_skill_status(&db, "skill-2", "retired")
            .await
            .is_err());
        assert!(list_skills(&db, Some("bogus"), 10).await.is_err());
    }

    #[tokio::test]
    async fn usage_tracking_and_review_thresholds() {
        let db = db().await;
        let row = create_skill(&db, skill("skill-use")).await.expect("create");
        assert_eq!(row.usage_count, 0);
        assert_eq!(row.confidence_score, 0.0);
        assert!((success_rate(&row) - 0.0).abs() < f64::EPSILON);
        // 5x100% success promotes confidence to 1.0.
        for _ in 0..5 {
            record_skill_use(&db, "skill-use", true).await.expect("use");
        }
        let row = get_skill(&db, "skill-use")
            .await
            .expect("get")
            .expect("row");
        assert_eq!(row.usage_count, 5);
        assert_eq!(row.success_count, 5);
        assert_eq!(row.failure_count, 0);
        assert!((row.confidence_score - 1.0).abs() < f64::EPSILON);
        assert!(row.last_used_at.is_some());
        assert!(!needs_review(&row));
        // Failures drag the rate under the threshold: flagged, then sustained
        // failure qualifies for auto-deprecation once published.
        for _ in 0..5 {
            record_skill_use(&db, "skill-use", false)
                .await
                .expect("use");
        }
        let row = get_skill(&db, "skill-use")
            .await
            .expect("get")
            .expect("row");
        assert_eq!(row.usage_count, 10);
        assert!((success_rate(&row) - 0.5).abs() < f64::EPSILON);
        assert!(needs_review(&row));
        assert!(!should_deprecate(&row));
        update_skill_status(&db, "skill-use", "validated")
            .await
            .expect("v");
        update_skill_status(&db, "skill-use", "published")
            .await
            .expect("p");
        let row = get_skill(&db, "skill-use")
            .await
            .expect("get")
            .expect("row");
        assert!(should_deprecate(&row));
        assert!(record_skill_use(&db, "missing", true).await.is_err());
    }

    #[tokio::test]
    async fn versioning_keeps_parent_for_rollback() {
        let db = db().await;
        create_skill(
            &db,
            NewSkill {
                version: "1".to_owned(),
                ..skill("skill-ver")
            },
        )
        .await
        .expect("create");
        assert_eq!(next_version("1"), "2");
        assert_eq!(next_version("0.1.0"), "0.1.1");
        assert_eq!(next_version("draft"), "draft.1");
        let child = create_skill_version(
            &db,
            "skill-ver",
            "1. Do it better. 2. Verify twice.",
            Some("Improved wording."),
        )
        .await
        .expect("version");
        assert_eq!(child.id, "skill-ver-v2");
        assert_eq!(child.version, "2");
        assert_eq!(child.parent_version.as_deref(), Some("skill-ver"));
        assert_eq!(child.status, "quarantined");
        assert_eq!(child.usage_count, 0);
        // Parent untouched: rollback is a status read away.
        let parent = get_skill(&db, "skill-ver")
            .await
            .expect("get")
            .expect("row");
        assert_eq!(parent.procedure_text, skill("skill-ver").procedure_text);
        assert_eq!(parent.version, "1");
        let versions = list_skill_versions(&db, "skill-ver")
            .await
            .expect("versions");
        assert_eq!(versions.len(), 2);
        assert!(create_skill_version(&db, "skill-ver", "", None)
            .await
            .is_err());
        assert!(create_skill_version(&db, "missing", "1. Go.", None)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn diff_is_deterministic_and_bounded() {
        let old = "1. Read the file.\n2. Hash it.\n3. Report.";
        let new = "1. Read the file.\n2. Hash it twice.\n3. Report.";
        let first = skill_diff(old, new);
        assert_eq!(first, skill_diff(old, new));
        assert!(first
            .iter()
            .any(|line| line.kind == "same" && line.text.contains("Read")));
        assert!(first
            .iter()
            .any(|line| line.kind == "removed" && line.text.contains("Hash it.")));
        assert!(first
            .iter()
            .any(|line| line.kind == "added" && line.text.contains("twice")));
        assert!(skill_diff("same", "same")
            .iter()
            .all(|line| line.kind == "same"));
    }

    #[tokio::test]
    async fn stale_published_skills_deprecate() {
        let db = db().await;
        create_skill(&db, skill("skill-stale"))
            .await
            .expect("create");
        update_skill_status(&db, "skill-stale", "validated")
            .await
            .expect("v");
        update_skill_status(&db, "skill-stale", "published")
            .await
            .expect("p");
        // Freshly published: nothing stale.
        let none = deprecate_stale_skills(&db, &now_iso(), 30)
            .await
            .expect("sweep");
        assert!(none.is_empty());
        // Far future without use: deprecated.
        let done = deprecate_stale_skills(&db, "2099-01-01T00:00:00Z", 30)
            .await
            .expect("sweep");
        assert_eq!(done, vec!["skill-stale".to_owned()]);
        let row = get_skill(&db, "skill-stale")
            .await
            .expect("get")
            .expect("row");
        assert_eq!(row.status, "deprecated");
    }

    #[tokio::test]
    async fn rejects_bad_input() {
        let db = db().await;
        assert!(create_skill(
            &db,
            NewSkill {
                id: String::new(),
                ..skill("x")
            }
        )
        .await
        .is_err());
        assert!(create_skill(
            &db,
            NewSkill {
                procedure_text: String::new(),
                ..skill("x")
            }
        )
        .await
        .is_err());
        assert!(create_skill(
            &db,
            NewSkill {
                permission_ceiling: "extreme".to_owned(),
                ..skill("x")
            }
        )
        .await
        .is_err());
        assert!(create_skill(
            &db,
            NewSkill {
                source_task_id: Some("missing".to_owned()),
                ..skill("x")
            }
        )
        .await
        .is_err());
        assert!(get_skill(&db, "").await.is_err());
    }
}
