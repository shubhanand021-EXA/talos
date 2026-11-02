//! Published-skill retrieval for context assembly and Tool-RAG.
//!
//! Registry skills reach the prompt through two paths:
//! - [`gather_published_skills`]: semantic recall over the `skill` vector
//!   entries, filtered to `status == "published"` and a permission ceiling
//!   covering the task risk, formatted as structured procedures (Top-K).
//! - [`published_skill_tools`]: the union of required tools across published
//!   skills, so Tool-RAG can boost member tools in candidate ranking.
//!
//! Quarantined skills are never retrievable here (verified-only recall plus
//! an explicit status check); promotion stays a human decision.

use std::collections::{BTreeSet, HashSet};

use evidence_store::EvidenceDb;
use talos_common::Result;

/// One published skill formatted for the tool layer.
#[derive(Debug, Clone, PartialEq)]
pub struct SkillSegment {
    /// `skill-{skill_id}` (stable segment id).
    pub id: String,
    /// Structured procedure body (sections, not raw text).
    pub body: String,
    /// Required tool ids (surfaced as `tool_ids` on the segment).
    pub tool_ids: Vec<String>,
    /// Recall score in [0, 1].
    pub score: f64,
}

/// Rank a permission ceiling against a task risk level: the skill covers the
/// task when `ceiling_rank >= risk_rank` (`low` < `medium` < `high` <
/// `irreversible`). Unknown values never cover.
#[must_use]
pub fn ceiling_covers(ceiling: &str, risk: &str) -> bool {
    fn rank(level: &str) -> u8 {
        match level.to_ascii_lowercase().as_str() {
            "low" => 0,
            "medium" => 1,
            "high" => 2,
            "irreversible" => 3,
            _ => u8::MAX,
        }
    }
    let (ceiling, risk) = (rank(ceiling), rank(risk));
    ceiling != u8::MAX && risk != u8::MAX && ceiling >= risk
}

/// Recall published skills matching `goal` (hybrid semantic recall over
/// verified `skill` entries, then status + ceiling filtering, Top-K).
/// `risk_ceiling` is the task risk level (`None` = no ceiling filter).
/// Deterministic for a fixed ledger (score order, id tiebreak).
pub async fn gather_published_skills(
    db: &EvidenceDb,
    goal: &str,
    risk_ceiling: Option<&str>,
    top_k: usize,
) -> Result<Vec<SkillSegment>> {
    if goal.trim().is_empty() {
        return Ok(Vec::new());
    }
    let top_k = top_k.clamp(1, 10);
    let filter = memory_indexer::MemoryFilter {
        source_types: vec!["skill".to_owned()],
        task_id: None,
        since: None,
        until: None,
        verified_only: true,
    };
    let hits = memory_indexer::hybrid_search(
        db,
        goal,
        top_k * 3,
        &filter,
        memory_indexer::RetrievalMode::Hybrid,
        0.5,
    )
    .await?;
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for hit in hits {
        if !seen.insert(hit.source_id.clone()) {
            continue;
        }
        let Some(skill) = evidence_store::get_skill(db, &hit.source_id).await? else {
            continue;
        };
        // Belt and suspenders: the index marks non-quarantined rows verified,
        // but only `published` may reach the prompt.
        if skill.status != "published" {
            continue;
        }
        if let Some(risk) = risk_ceiling {
            if !ceiling_covers(&skill.permission_ceiling, risk) {
                continue;
            }
        }
        let tools: Vec<String> = serde_json::from_str(&skill.required_tools).unwrap_or_default();
        out.push(SkillSegment {
            id: format!("skill-{}", skill.id),
            body: format_skill_body(&skill, &tools),
            tool_ids: tools,
            score: hit.score,
        });
        if out.len() >= top_k {
            break;
        }
    }
    Ok(out)
}

/// Structured procedure body: titled sections with version, confidence
/// inputs, and required tools — never the raw row dump.
fn format_skill_body(skill: &evidence_store::SkillRow, tools: &[String]) -> String {
    format!(
        "# Skill: {} (v{})\n## When to use\n{}\n## Steps\n{}\n## Required tools\n{}",
        skill.name,
        skill.version,
        skill.description,
        skill.procedure_text,
        if tools.is_empty() {
            "(none)".to_owned()
        } else {
            tools.join(", ")
        }
    )
}

/// Union of required tools across published skills (sorted, deduped) for
/// Tool-RAG member boosts.
pub async fn published_skill_tools(db: &EvidenceDb) -> Result<HashSet<String>> {
    let rows = evidence_store::list_skills(db, Some("published"), 500).await?;
    let mut tools = HashSet::new();
    for row in rows {
        let required: Vec<String> = serde_json::from_str(&row.required_tools).unwrap_or_default();
        tools.extend(required);
    }
    Ok(tools)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn db() -> EvidenceDb {
        EvidenceDb::connect_memory().await.expect("memory db")
    }

    async fn publish_skill(db: &EvidenceDb, id: &str, ceiling: &str) {
        evidence_store::create_skill(
            db,
            evidence_store::NewSkill {
                id: id.to_owned(),
                name: "read manifest".to_owned(),
                description: "Read the project manifest and report its hash.".to_owned(),
                procedure_text: "1. Call `fs.read` (path=manifest.json).".to_owned(),
                source_task_id: None,
                version: "1".to_owned(),
                test_fixture_ref: None,
                required_tools: vec!["fs.read".to_owned()],
                permission_ceiling: ceiling.to_owned(),
            },
        )
        .await
        .expect("create");
        evidence_store::update_skill_status(db, id, "validated")
            .await
            .expect("v");
        evidence_store::update_skill_status(db, id, "published")
            .await
            .expect("p");
        // Index the row so hybrid recall can find it.
        memory_indexer::index_incremental(db).await.expect("index");
    }

    #[test]
    fn ceiling_covering_is_conservative() {
        assert!(ceiling_covers("low", "low"));
        assert!(ceiling_covers("medium", "low"));
        assert!(!ceiling_covers("low", "high"));
        assert!(ceiling_covers("irreversible", "irreversible"));
        assert!(!ceiling_covers("bogus", "low"));
        assert!(!ceiling_covers("low", "bogus"));
    }

    #[tokio::test]
    async fn published_skill_appears_in_retrieval() {
        let db = db().await;
        publish_skill(&db, "skill-retrieve", "low").await;
        let segments = gather_published_skills(&db, "read the project manifest", None, 3)
            .await
            .expect("gather");
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].id, "skill-skill-retrieve");
        assert!(segments[0].body.contains("# Skill: read manifest (v1)"));
        assert!(segments[0].body.contains("## Steps"));
        assert!(segments[0].body.contains("## Required tools"));
        assert_eq!(segments[0].tool_ids, vec!["fs.read".to_owned()]);
        let tools = published_skill_tools(&db).await.expect("tools");
        assert!(tools.contains("fs.read"));
    }

    #[tokio::test]
    async fn quarantined_skill_never_retrieved() {
        let db = db().await;
        evidence_store::create_skill(
            &db,
            evidence_store::NewSkill {
                id: "skill-quar".to_owned(),
                name: "read manifest".to_owned(),
                description: "Read the project manifest.".to_owned(),
                procedure_text: "1. Call `fs.read`.".to_owned(),
                source_task_id: None,
                version: "1".to_owned(),
                test_fixture_ref: None,
                required_tools: vec!["fs.read".to_owned()],
                permission_ceiling: "low".to_owned(),
            },
        )
        .await
        .expect("create");
        memory_indexer::index_incremental(&db).await.expect("index");
        let segments = gather_published_skills(&db, "read the project manifest", None, 3)
            .await
            .expect("gather");
        assert!(segments.is_empty());
        assert!(published_skill_tools(&db).await.expect("tools").is_empty());
    }

    #[tokio::test]
    async fn retrieval_respects_permission_ceiling() {
        let db = db().await;
        publish_skill(&db, "skill-low", "low").await;
        // A high-risk task is not covered by a low-ceiling skill.
        let blocked = gather_published_skills(&db, "read the project manifest", Some("high"), 3)
            .await
            .expect("gather");
        assert!(blocked.is_empty());
        // A low-risk task is covered.
        let allowed = gather_published_skills(&db, "read the project manifest", Some("low"), 3)
            .await
            .expect("gather");
        assert_eq!(allowed.len(), 1);
    }
}
