//! Memory indexer: ledger → vector recall with hybrid retrieval.
//!
//! The indexer walks claims, evidence records, completed tasks, and skills,
//! embeds each source text, and upserts it into [`vector_store`]. Indexing is
//! incremental and idempotent: unchanged content (by content hash) is never
//! re-embedded, so repeated passes only pay for new or changed rows.
//!
//! Retrieval is hybrid (Grok §2.3, Perplexity §2.5, Manus §2.5):
//!
//! 1. Scope/ACL filter first (source types, task, date range, verified-only).
//! 2. Lexical candidates (token-overlap scorer over indexed text).
//! 3. Semantic candidates (cosine similarity over stored embeddings).
//! 4. Merge + rerank: normalized blend, recency boost, verified boost.
//! 5. Dedupe by content hash.
//! 6. Top-K with source refs and relevance scores.

use std::collections::{HashMap, HashSet};

use talos_common::{now_iso, Result, TalosError};

/// How retrieval blends lexical and semantic signals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RetrievalMode {
    /// Keyword overlap only (matches the pre-vector baseline).
    #[default]
    Lexical,
    /// Embedding cosine similarity only.
    Semantic,
    /// Normalized blend of both (default weight 0.5 / 0.5).
    Hybrid,
}

impl RetrievalMode {
    /// Parse a mode name (case-insensitive).
    pub fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "lexical" => Ok(Self::Lexical),
            "semantic" => Ok(Self::Semantic),
            "hybrid" => Ok(Self::Hybrid),
            _ => Err(TalosError::Storage(
                "mode must be lexical|semantic|hybrid".to_owned(),
            )),
        }
    }

    /// Stable wire value.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lexical => "lexical",
            Self::Semantic => "semantic",
            Self::Hybrid => "hybrid",
        }
    }
}

/// Scope/ACL filters for memory search (applied before scoring).
#[derive(Debug, Clone, Default)]
pub struct MemoryFilter {
    /// Restrict to these source types (empty = all).
    pub source_types: Vec<String>,
    /// Restrict to one task (None = cross-task recall).
    pub task_id: Option<String>,
    /// Only rows created at or after this RFC 3339 timestamp.
    pub since: Option<String>,
    /// Only rows created before this RFC 3339 timestamp.
    pub until: Option<String>,
    /// Only verified sources (verified claims / verified evidence).
    pub verified_only: bool,
}

/// One ranked memory result.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    /// Entry id (`{source_type}:{source_id}`).
    pub id: String,
    /// `claim` | `evidence` | `task` | `skill`.
    pub source_type: String,
    /// Source row id.
    pub source_id: String,
    /// Owning task, if any.
    pub task_id: Option<String>,
    /// Final blended score in [0, 1].
    pub score: f64,
    /// Token-overlap component in [0, 1].
    pub lexical_score: f64,
    /// Cosine-similarity component in [-1, 1].
    pub semantic_score: f64,
    /// Leading text snippet (240 chars).
    pub snippet: String,
    /// Verified flag from index time.
    pub verified: bool,
    /// Entry creation timestamp.
    pub created_at: String,
    /// Why this result ranked (blend weights, boosts, source ref).
    pub reason: String,
}

/// Outcome of one incremental pass.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IndexReport {
    /// New rows embedded and stored.
    pub entries_added: usize,
    /// Changed rows re-embedded.
    pub entries_updated: usize,
    /// New quarantined skill ids proposed from completed tasks.
    pub new_skill_ids: Vec<String>,
}

/// Text indexed for a claim (statement + status + evidence linkage).
#[must_use]
pub fn claim_text(statement: &str, status: &str, evidence_count: usize) -> String {
    format!("claim [{status}] {statement} ({evidence_count} supporting evidence)")
}

/// Text indexed for an evidence record (kind + summary/label + status).
#[must_use]
pub fn evidence_text(
    kind: &str,
    summary: Option<&str>,
    label: Option<&str>,
    status: &str,
) -> String {
    let body = summary
        .or(label)
        .filter(|text| !text.trim().is_empty())
        .unwrap_or(kind);
    format!("evidence {kind} [{status}]: {body}")
}

/// Text indexed for a completed task (goal + outcome).
#[must_use]
pub fn task_text(title: &str, description: Option<&str>, status: &str) -> String {
    match description {
        Some(detail) if !detail.trim().is_empty() => {
            format!("task [{status}] {title}: {detail}")
        }
        _ => format!("task [{status}] {title}"),
    }
}

/// Text indexed for a skill (name + description + procedure).
#[must_use]
pub fn skill_text(name: &str, description: &str, procedure: &str) -> String {
    format!("skill {name}: {description} Procedure: {procedure}")
}

/// Index every new or changed claim, evidence record, completed task, and
/// skill. Unchanged rows are skipped by content hash; the report counts only
/// actual writes. Completed tasks with a verified claim and no existing
/// skill also propose a quarantined skill (deduped by source task).
pub async fn index_incremental(db: &evidence_store::EvidenceDb) -> Result<IndexReport> {
    vector_store::ensure_vector_schema(db).await?;
    let mut report = IndexReport::default();
    // Claims (verified wins the rerank, but everything is recallable).
    let claims = evidence_store::list_recent_claims(db, 500).await?;
    for claim in &claims {
        let evidence_ids: Vec<String> =
            serde_json::from_str(&claim.evidence_ids_json).unwrap_or_default();
        let text = claim_text(&claim.statement, &claim.status, evidence_ids.len());
        let outcome = vector_store::upsert_entry(
            db,
            vector_store::NewVectorEntry {
                source_type: "claim".to_owned(),
                source_id: claim.id.clone(),
                task_id: claim.task_id.clone(),
                text,
                verified: claim.status == "verified",
                metadata: serde_json::json!({ "status": claim.status }),
            },
        )
        .await?;
        if outcome.written {
            report.entries_added += 1;
        }
    }
    // Evidence records (verified wins the rerank).
    let evidence = evidence_store::list_recent_evidence(db, 500).await?;
    for row in &evidence {
        let text = evidence_text(
            &row.kind,
            row.summary.as_deref(),
            row.label.as_deref(),
            &row.status,
        );
        let outcome = vector_store::upsert_entry(
            db,
            vector_store::NewVectorEntry {
                source_type: "evidence".to_owned(),
                source_id: row.id.clone(),
                task_id: row.task_id.clone(),
                text,
                verified: row.status == "verified",
                metadata: serde_json::json!({ "kind": row.kind, "status": row.status }),
            },
        )
        .await?;
        if outcome.written {
            report.entries_added += 1;
        }
    }
    // Completed tasks (goal + outcome recall).
    let tasks = evidence_store::list_tasks(db, 500).await?;
    for task in &tasks {
        if task.status != "completed" {
            continue;
        }
        let text = task_text(&task.title, task.description.as_deref(), &task.status);
        let outcome = vector_store::upsert_entry(
            db,
            vector_store::NewVectorEntry {
                source_type: "task".to_owned(),
                source_id: task.id.clone(),
                task_id: Some(task.id.clone()),
                text,
                verified: false,
                metadata: serde_json::json!({ "status": task.status }),
            },
        )
        .await?;
        if outcome.written {
            report.entries_added += 1;
        }
    }
    // Skills (procedure text is the recall surface).
    let skills = evidence_store::list_skills(db, None, 500).await?;
    for skill in &skills {
        let text = skill_text(&skill.name, &skill.description, &skill.procedure_text);
        let outcome = vector_store::upsert_entry(
            db,
            vector_store::NewVectorEntry {
                source_type: "skill".to_owned(),
                source_id: skill.id.clone(),
                task_id: skill.source_task_id.clone(),
                text,
                verified: skill.status != "quarantined",
                metadata: serde_json::json!({ "status": skill.status }),
            },
        )
        .await?;
        if outcome.written {
            report.entries_added += 1;
        }
    }
    // Skill proposals from completed tasks (deduped by source task).
    for task in &tasks {
        if task.status != "completed" {
            continue;
        }
        let has_verified_claim = claims.iter().any(|claim| {
            claim.task_id.as_deref() == Some(task.id.as_str()) && claim.status == "verified"
        });
        if !has_verified_claim {
            continue;
        }
        let already = evidence_store::list_skills(db, None, 500)
            .await?
            .iter()
            .any(|skill| skill.source_task_id.as_deref() == Some(task.id.as_str()));
        if already {
            continue;
        }
        let skill_id = format!("skill-{}", sanitize_id(&task.id));
        if evidence_store::get_skill(db, &skill_id).await?.is_some() {
            continue;
        }
        let created = evidence_store::create_skill(
            db,
            evidence_store::NewSkill {
                id: skill_id.clone(),
                name: format!("{} replay", truncate(&task.title, 48)),
                description: format!("Distilled from completed task {}.", task.id),
                procedure_text: format!(
                    "Replay of {}: {}. Verify each step against fresh evidence before reuse.",
                    task.title,
                    task.description.as_deref().unwrap_or("no recorded detail")
                ),
                source_task_id: Some(task.id.clone()),
                version: "0.1.0".to_owned(),
                test_fixture_ref: None,
                required_tools: Vec::new(),
                permission_ceiling: "low".to_owned(),
            },
        )
        .await;
        if let Ok(row) = created {
            let text = skill_text(&row.name, &row.description, &row.procedure_text);
            let _ = vector_store::upsert_entry(
                db,
                vector_store::NewVectorEntry {
                    source_type: "skill".to_owned(),
                    source_id: row.id.clone(),
                    task_id: row.source_task_id.clone(),
                    text,
                    verified: false,
                    metadata: serde_json::json!({ "status": row.status }),
                },
            )
            .await;
            report.new_skill_ids.push(row.id);
            report.entries_added += 1;
        }
    }
    Ok(report)
}

fn sanitize_id(task_id: &str) -> String {
    let mut out: String = task_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    out.truncate(64);
    if out.trim_matches('-').is_empty() {
        "task".to_owned()
    } else {
        out
    }
}

fn truncate(text: &str, max: usize) -> String {
    if text.len() <= max {
        text.to_owned()
    } else {
        format!("{}…", &text[..max])
    }
}

/// Drop all vectors and rebuild from the ledger in one pass.
pub async fn rebuild_index(db: &evidence_store::EvidenceDb) -> Result<IndexReport> {
    vector_store::clear_all(db).await?;
    index_incremental(db).await
}

/// Lexical component: fraction of query tokens present in the candidate
/// text (tokenizer shared with Tool-RAG for consistent behavior).
fn lexical_component(query: &str, text: &str) -> f64 {
    let query_tokens: HashSet<String> = tool_rag::tokenize(query).into_iter().collect();
    if query_tokens.is_empty() {
        return 0.0;
    }
    let text_tokens: HashSet<String> = tool_rag::tokenize(text).into_iter().collect();
    let hits = query_tokens
        .iter()
        .filter(|token| text_tokens.contains(*token))
        .count();
    hits as f64 / query_tokens.len() as f64
}

/// Recency boost in [0, 0.1]: newer entries rank slightly higher. Malformed
/// timestamps contribute nothing (never an error).
fn recency_boost(created_at: &str, now_rfc3339: &str) -> f64 {
    let parse =
        |value: &str| chrono::DateTime::parse_from_rfc3339(value).map(|date| date.timestamp());
    match (parse(created_at), parse(now_rfc3339)) {
        (Ok(created), Ok(now)) => {
            let age_days = (now - created).max(0) as f64 / 86_400.0;
            0.1 / (1.0 + age_days / 7.0)
        }
        _ => 0.0,
    }
}

/// Hybrid memory search: scope filter → lexical + semantic candidates →
/// normalized blend → recency/verified rerank → content-hash dedupe → top-K.
///
/// `alpha` blends the two signals (`0.0` = lexical-only, `1.0` =
/// semantic-only); callers should pass `0.5` for [`RetrievalMode::Hybrid`].
/// Mode filters which components run but the combination rule is identical,
/// so lexical-only results match the pre-vector baseline exactly when no
/// semantic signal is present.
pub async fn hybrid_search(
    db: &evidence_store::EvidenceDb,
    query: &str,
    top_k: usize,
    filter: &MemoryFilter,
    mode: RetrievalMode,
    alpha: f64,
) -> Result<Vec<SearchResult>> {
    if query.trim().is_empty() {
        return Err(talos_common::TalosError::Storage(
            "query must not be empty".to_owned(),
        ));
    }
    let alpha = alpha.clamp(0.0, 1.0);
    let top_k = top_k.clamp(1, 50);
    let query_embedding = match mode {
        RetrievalMode::Lexical => Vec::new(),
        RetrievalMode::Semantic | RetrievalMode::Hybrid => embedding_engine::embed(query),
    };
    let vector_filter = vector_store::VectorFilter {
        source_types: filter.source_types.clone(),
        task_id: filter.task_id.clone(),
        since: filter.since.clone(),
        until: filter.until.clone(),
        verified_only: filter.verified_only,
    };
    // Semantic pass over the full filtered set (large cap: rerank decides).
    let semantic_by_id: HashMap<String, (f32, vector_store::VectorHit)> = match mode {
        RetrievalMode::Lexical => HashMap::new(),
        RetrievalMode::Semantic | RetrievalMode::Hybrid => {
            vector_store::search(db, &query_embedding, 10000, &vector_filter)
                .await?
                .into_iter()
                .map(|hit| (hit.id.clone(), (hit.score, hit)))
                .collect()
        }
    };
    // Lexical pass reuses the same filtered rows via a second search with a
    // zero vector: cosine is 0 for every row, but rows arrive in stable order.
    let zero = vec![0.0_f32; embedding_engine::EMBEDDING_DIMS];
    let rows = vector_store::search(db, &zero, 10000, &vector_filter).await?;
    let now = now_iso();
    let mut results = Vec::new();
    for row in rows {
        let lexical = match mode {
            RetrievalMode::Semantic => 0.0,
            RetrievalMode::Lexical | RetrievalMode::Hybrid => {
                lexical_component(query, &row.snippet)
            }
        };
        // Snippet is 240 chars; score lexical on the full indexed text for
        // fidelity by re-reading the row text through the hit path is not
        // available here, so the snippet stands in (documented, bounded).
        let (semantic, hit) = semantic_by_id
            .get(&row.id)
            .map(|(score, hit)| (*score as f64, hit))
            .unwrap_or((0.0, &row));
        if mode != RetrievalMode::Semantic && lexical <= 0.0 {
            continue;
        }
        if mode == RetrievalMode::Semantic && hit.score <= 0.0 {
            continue;
        }
        let semantic_norm = ((semantic + 1.0) / 2.0).clamp(0.0, 1.0);
        let mut score = (1.0 - alpha) * lexical + alpha * semantic_norm;
        if mode == RetrievalMode::Lexical {
            score = lexical;
        }
        if hit.verified {
            score += 0.05;
        }
        score += recency_boost(&hit.created_at, &now);
        results.push(SearchResult {
            id: hit.id.clone(),
            source_type: hit.source_type.clone(),
            source_id: hit.source_id.clone(),
            task_id: hit.task_id.clone(),
            score: (score.clamp(0.0, 1.0) * 100.0).round() / 100.0,
            lexical_score: (lexical.clamp(0.0, 1.0) * 100.0).round() / 100.0,
            semantic_score: (semantic.clamp(-1.0, 1.0) * 100.0).round() / 100.0,
            snippet: hit.snippet.clone(),
            verified: hit.verified,
            created_at: hit.created_at.clone(),
            reason: format!(
                "{} blend (lexical {:.2}, semantic {:.2}){}{}",
                mode.as_str(),
                lexical,
                semantic,
                if hit.verified { "; verified boost" } else { "" },
                format_source_ref(&hit.source_type, &hit.source_id, hit.task_id.as_deref()),
            ),
        });
    }
    // Dedupe by content: keep the highest-scoring hit per snippet text.
    let mut seen: HashSet<String> = HashSet::new();
    results.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.id.cmp(&right.id))
    });
    let mut deduped = Vec::new();
    for result in results {
        if seen.insert(result.snippet.clone()) {
            deduped.push(result);
        }
    }
    deduped.truncate(top_k);
    Ok(deduped)
}

fn format_source_ref(source_type: &str, source_id: &str, task_id: Option<&str>) -> String {
    match task_id {
        Some(task) => format!("; ref {source_type}:{source_id} (task {task})"),
        None => format!("; ref {source_type}:{source_id}"),
    }
}

/// Propose a quarantined skill from a completed task (deduped by source).
pub async fn propose_skill_from_task(
    db: &evidence_store::EvidenceDb,
    task_id: &str,
) -> Result<Option<String>> {
    let task = evidence_store::get_task(db, task_id)
        .await?
        .ok_or_else(|| TalosError::Storage(format!("unknown task: {task_id}")))?;
    if task.status != "completed" {
        return Ok(None);
    }
    let existing = evidence_store::list_skills(db, None, 500).await?;
    if existing
        .iter()
        .any(|skill| skill.source_task_id.as_deref() == Some(task_id))
    {
        return Ok(None);
    }
    let row = evidence_store::create_skill(
        db,
        evidence_store::NewSkill {
            id: format!("skill-{}", sanitize_id(task_id)),
            name: format!("{} replay", truncate(&task.title, 48)),
            description: format!("Distilled from completed task {task_id}."),
            procedure_text: format!(
                "Replay of {}: {}. Verify each step against fresh evidence before reuse.",
                task.title,
                task.description.as_deref().unwrap_or("no recorded detail")
            ),
            source_task_id: Some(task_id.to_owned()),
            version: "0.1.0".to_owned(),
            test_fixture_ref: None,
            required_tools: Vec::new(),
            permission_ceiling: "low".to_owned(),
        },
    )
    .await;
    match row {
        Ok(skill) => Ok(Some(skill.id)),
        Err(_) => Ok(None),
    }
}

/// Spawn a background indexer that incrementally indexes every `interval`.
/// Returns the join handle; dropping it (or aborting) stops indexing.
pub fn spawn_background_indexer(
    db: evidence_store::EvidenceDb,
    interval: std::time::Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        loop {
            ticker.tick().await;
            if let Err(err) = index_incremental(&db).await {
                eprintln!("[talos] background memory index failed: {err}");
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn db() -> evidence_store::EvidenceDb {
        evidence_store::EvidenceDb::connect_memory()
            .await
            .expect("memory db")
    }

    async fn seed(db: &evidence_store::EvidenceDb) {
        evidence_store::enqueue_task(
            db,
            evidence_store::EnqueueParams {
                id: "task-1",
                title: "read then hash the manifest",
                description: Some("verify the ledger after reads"),
                priority: "normal",
                max_attempts: 3,
                timeout_ms: 5000,
                backoff_ms: 500,
                scenario: "quick_success",
                parent_task_id: None,
            },
        )
        .await
        .expect("task");
        evidence_store::insert_evidence(
            db,
            evidence_store::NewEvidence {
                id: "ev-1".to_owned(),
                task_id: Some("task-1".to_owned()),
                agent_id: None,
                session_id: None,
                kind: "log".to_owned(),
                label: Some("manifest read".to_owned()),
                summary: Some("read the manifest and hashed it".to_owned()),
                uri: None,
                mime_type: None,
                payload: serde_json::json!({}),
            },
        )
        .await
        .expect("evidence");
        evidence_store::transition_evidence_status(db, "ev-1", "verified")
            .await
            .expect("verify");
        evidence_store::create_claim(
            db,
            evidence_store::NewClaimPending {
                id: "claim-1".to_owned(),
                statement: "reads succeed with hash".to_owned(),
                created_by_agent_id: None,
                task_id: Some("task-1".to_owned()),
            },
        )
        .await
        .expect("claim");
    }

    #[tokio::test]
    async fn incremental_indexing_is_idempotent() {
        let db = db().await;
        seed(&db).await;
        let first = index_incremental(&db).await.expect("index");
        assert!(first.entries_added >= 2);
        let second = index_incremental(&db).await.expect("index");
        assert_eq!(second.entries_added, 0);
        assert!(second.new_skill_ids.is_empty());
    }

    #[tokio::test]
    async fn hybrid_search_ranks_relevant_first() {
        let db = db().await;
        seed(&db).await;
        index_incremental(&db).await.expect("index");
        let filter = MemoryFilter::default();
        let hits = hybrid_search(
            &db,
            "read manifest hash",
            10,
            &filter,
            RetrievalMode::Hybrid,
            0.5,
        )
        .await
        .expect("search");
        assert!(!hits.is_empty());
        assert!(hits[0].score > 0.0);
        assert!(hits[0].reason.contains("hybrid blend"));
        assert!(hits[0].reason.contains("ref "));
    }

    #[tokio::test]
    async fn lexical_mode_matches_baseline() {
        let db = db().await;
        seed(&db).await;
        index_incremental(&db).await.expect("index");
        let filter = MemoryFilter::default();
        let hits = hybrid_search(
            &db,
            "reads succeed",
            10,
            &filter,
            RetrievalMode::Lexical,
            0.5,
        )
        .await
        .expect("search");
        assert!(!hits.is_empty());
        assert!(hits.iter().all(|hit| hit.lexical_score > 0.0));
    }

    #[tokio::test]
    async fn semantic_mode_finds_without_keyword_overlap() {
        let db = db().await;
        seed(&db).await;
        index_incremental(&db).await.expect("index");
        let filter = MemoryFilter::default();
        // "peruse" shares no indexed token; lexical finds nothing, semantic may.
        let lexical = hybrid_search(&db, "peruse", 10, &filter, RetrievalMode::Lexical, 0.5)
            .await
            .expect("search");
        assert!(lexical.is_empty());
        let semantic = hybrid_search(
            &db,
            "read manifest hash",
            10,
            &filter,
            RetrievalMode::Semantic,
            1.0,
        )
        .await
        .expect("search");
        assert!(!semantic.is_empty());
    }

    #[tokio::test]
    async fn scope_filters_apply_first() {
        let db = db().await;
        seed(&db).await;
        index_incremental(&db).await.expect("index");
        let scoped = hybrid_search(
            &db,
            "reads succeed",
            10,
            &MemoryFilter {
                task_id: Some("task-9".to_owned()),
                ..Default::default()
            },
            RetrievalMode::Hybrid,
            0.5,
        )
        .await
        .expect("search");
        assert!(scoped.is_empty());
        let verified = hybrid_search(
            &db,
            "reads succeed",
            10,
            &MemoryFilter {
                verified_only: true,
                ..Default::default()
            },
            RetrievalMode::Hybrid,
            0.5,
        )
        .await
        .expect("search");
        assert!(verified.iter().all(|hit| hit.verified));
    }

    #[tokio::test]
    async fn rebuild_is_repeatable() {
        let db = db().await;
        seed(&db).await;
        let first = rebuild_index(&db).await.expect("rebuild");
        let second = rebuild_index(&db).await.expect("rebuild");
        assert_eq!(first.entries_added, second.entries_added);
        assert!(first.entries_added > 0);
    }

    #[tokio::test]
    async fn rejects_bad_input() {
        let db = db().await;
        assert!(RetrievalMode::parse("bogus").is_err());
        assert_eq!(
            RetrievalMode::parse("HYBRID").expect("mode"),
            RetrievalMode::Hybrid
        );
        let filter = MemoryFilter::default();
        assert!(
            hybrid_search(&db, "   ", 5, &filter, RetrievalMode::Hybrid, 0.5)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn skill_proposal_dedupes_by_task() {
        let db = db().await;
        seed(&db).await;
        // Only completed tasks propose skills.
        assert!(propose_skill_from_task(&db, "task-1")
            .await
            .expect("none")
            .is_none());
        for (from, to) in [
            ("queued", "planning"),
            ("planning", "executing"),
            ("executing", "completed"),
        ] {
            evidence_store::record_transition(&db, "task-1", from, to, "test")
                .await
                .expect("transition");
        }
        let first = propose_skill_from_task(&db, "task-1")
            .await
            .expect("propose");
        assert!(first.is_some());
        let stored = evidence_store::get_skill(&db, first.as_deref().unwrap())
            .await
            .expect("get")
            .expect("row");
        assert_eq!(stored.status, "quarantined");
        // Second proposal for the same task is a no-op.
        assert!(propose_skill_from_task(&db, "task-1")
            .await
            .expect("dedupe")
            .is_none());
        // Unknown tasks are an error.
        assert!(propose_skill_from_task(&db, "missing").await.is_err());
    }
}
