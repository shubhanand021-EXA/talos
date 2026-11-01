//! Task-scoped context gathering from every Talos source.
//!
//! The gatherer reads the ledger (task contract, evidence, memory claims,
//! session events) and ranks tools through `tool_rag`. Safety rules and tool
//! schemas arrive from the caller (desktop owns the registry + envelope);
//! gathered text is layered by the assembler, never executed.

use std::collections::HashMap;

use talos_common::Result;

/// One layered text piece for assembly.
#[derive(Debug, Clone, PartialEq)]
pub struct SegmentInput {
    /// Stable segment id.
    pub id: String,
    /// Layer name (`safety`, `system`, `task`, `session`, `evidence`, `tool`, `scratchpad`).
    pub layer: &'static str,
    /// Text body.
    pub body: String,
    /// Protected segments survive compaction verbatim.
    pub protected: bool,
    /// Safety-critical segments are never compacted or dropped.
    pub safety_critical: bool,
    /// Evidence ids backing this piece.
    pub evidence_refs: Vec<String>,
    /// Tool ids referenced by this piece.
    pub tool_ids: Vec<String>,
}

/// One ranked tool for the prompt catalog.
#[derive(Debug, Clone, PartialEq)]
pub struct RankedTool {
    /// Tool id.
    pub tool_id: String,
    /// Rank score.
    pub score: f64,
    /// Why it ranked.
    pub reason: String,
}

/// Tool schema entry for `LlmRequest.tools`.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolEntry {
    /// Tool name.
    pub name: String,
    /// Human description.
    pub description: String,
    /// JSON-schema input shape.
    pub input_schema: serde_json::Value,
}

/// One bounded conversation turn.
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryTurn {
    /// `system` | `user` | `assistant` | `tool`.
    pub role: String,
    /// Turn text.
    pub content: String,
}

/// Everything the assembler needs, gathered task-first.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GatheredContext {
    /// Owning task.
    pub task_id: String,
    /// Calling agent.
    pub agent_id: String,
    /// Session scope.
    pub session_id: String,
    /// Task goal (intent).
    pub goal: String,
    /// Acceptance criteria.
    pub acceptance: Vec<String>,
    /// Task constraints.
    pub constraints: Vec<String>,
    /// Task title / status / priority for the contract block.
    pub task_title: String,
    /// Task status.
    pub task_status: String,
    /// Task priority.
    pub task_priority: String,
    /// Verbatim safety rules (never compressed downstream).
    pub safety_rules: Vec<String>,
    /// Layered text pieces (evidence, session, scratchpad, …).
    pub segments: Vec<SegmentInput>,
    /// Ranked tools for the catalog.
    pub ranked_tools: Vec<RankedTool>,
    /// Schemas for the tools above.
    pub tool_entries: Vec<ToolEntry>,
    /// Bounded conversation history.
    pub history: Vec<HistoryTurn>,
    /// The immediate instruction.
    pub instruction: String,
    /// Chain head hash at gather time (None when the log is empty).
    pub chain_head_hash: Option<String>,
}

/// Task contract block for the assembler.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskContract {
    /// Task id.
    pub task_id: String,
    /// Human-readable title.
    pub title: String,
    /// Lifecycle status.
    pub status: String,
    /// Scheduling priority.
    pub priority: String,
}

/// Load the task contract. Unknown tasks are an error (no anonymous assembly).
pub async fn gather_task_contract(
    db: &evidence_store::EvidenceDb,
    task_id: &str,
) -> Result<TaskContract> {
    let row = evidence_store::get_task(db, task_id)
        .await?
        .ok_or_else(|| talos_common::TalosError::Storage(format!("unknown task: {task_id}")))?;
    Ok(TaskContract {
        task_id: row.id,
        title: row.title,
        status: row.status,
        priority: row.priority,
    })
}

/// Collect verified-first evidence pieces for a task (bounded).
pub async fn gather_evidence(
    db: &evidence_store::EvidenceDb,
    task_id: &str,
    limit: usize,
) -> Result<Vec<SegmentInput>> {
    let rows = evidence_store::list_recent_evidence(db, limit.clamp(1, 200) as i64).await?;
    let mut pieces: Vec<(bool, SegmentInput)> = Vec::new();
    for row in rows {
        if row.task_id.as_deref() != Some(task_id) {
            continue;
        }
        let verified = row.status == "verified";
        let body = row
            .summary
            .clone()
            .or(row.label.clone())
            .unwrap_or_else(|| row.kind.clone());
        pieces.push((
            verified,
            SegmentInput {
                id: format!("evidence-{}", row.id),
                layer: "evidence",
                body: format!("{} [{}:{}]", body, row.kind, row.id),
                protected: false,
                safety_critical: false,
                evidence_refs: vec![row.id],
                tool_ids: Vec::new(),
            },
        ));
    }
    // Verified first (stable: verified, then id order), then cap.
    pieces.sort_by_key(|(verified, piece)| (!*verified, piece.id.clone()));
    Ok(pieces.into_iter().map(|(_, piece)| piece).collect())
}

/// Collect memory facts from task claims (verified first, bounded).
pub async fn gather_memory(
    db: &evidence_store::EvidenceDb,
    task_id: &str,
    limit: usize,
) -> Result<Vec<SegmentInput>> {
    let rows = evidence_store::list_recent_claims(db, limit.clamp(1, 200) as i64).await?;
    let mut pieces: Vec<(bool, SegmentInput)> = Vec::new();
    for row in rows {
        if row.task_id.as_deref() != Some(task_id) {
            continue;
        }
        let verified = row.status == "verified";
        pieces.push((
            verified,
            SegmentInput {
                id: format!("memory-{}", row.id),
                layer: "session",
                body: format!("{} [claim:{}]", row.statement, row.id),
                protected: false,
                safety_critical: false,
                evidence_refs: Vec::new(),
                tool_ids: Vec::new(),
            },
        ));
    }
    pieces.sort_by_key(|(verified, piece)| (!*verified, piece.id.clone()));
    Ok(pieces.into_iter().map(|(_, piece)| piece).collect())
}

/// Collect recent task-scoped session events (bounded, newest last).
pub async fn gather_session(
    db: &evidence_store::EvidenceDb,
    task_id: &str,
    limit: usize,
) -> Result<Vec<SegmentInput>> {
    let rows = evidence_store::list_recent_log(db, limit.clamp(1, 200) as i64).await?;
    let mut pieces = Vec::new();
    for row in rows.into_iter().rev() {
        if !row.payload_json.contains(task_id) {
            continue;
        }
        pieces.push(SegmentInput {
            id: format!("session-{}", row.id),
            layer: "session",
            body: format!("{}: {}", row.kind, row.payload_json),
            protected: false,
            safety_critical: false,
            evidence_refs: Vec::new(),
            tool_ids: Vec::new(),
        });
    }
    Ok(pieces)
}

/// Rank tools for an intent over prebuilt index docs + receipt stats.
pub fn rank_tools(
    docs: &[tool_rag::IndexedTool],
    stats: &HashMap<String, tool_rag::ReceiptStats>,
    intent: &str,
    top_k: usize,
) -> std::result::Result<Vec<RankedTool>, String> {
    let query = tool_rag::validate_query(intent, Vec::new(), None, false, Some(top_k))
        .map_err(|err| err.to_string())?;
    Ok(
        tool_rag::rank(&query, docs, stats, &tool_rag::LocalLexicalProvider)
            .into_iter()
            .map(|candidate| RankedTool {
                tool_id: candidate.tool_id,
                score: candidate.score,
                reason: candidate.reason,
            })
            .collect(),
    )
}

/// Current hash-chain head (None when the log is empty).
pub async fn chain_head_hash(db: &evidence_store::EvidenceDb) -> Result<Option<String>> {
    Ok(evidence_store::get_integrity_report(db)
        .await?
        .chain
        .last_hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn db() -> evidence_store::EvidenceDb {
        evidence_store::EvidenceDb::connect_memory()
            .await
            .expect("memory db")
    }

    async fn seeded() -> evidence_store::EvidenceDb {
        let db = db().await;
        evidence_store::enqueue_task(
            &db,
            evidence_store::EnqueueParams {
                id: "task-g1",
                title: "gather me",
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
        .unwrap();
        evidence_store::insert_evidence(
            &db,
            evidence_store::NewEvidence {
                id: "ev-1".to_owned(),
                task_id: Some("task-g1".to_owned()),
                agent_id: Some("agent-1".to_owned()),
                session_id: Some("session-local".to_owned()),
                kind: "tool_receipt".to_owned(),
                label: Some("fs.read ok".to_owned()),
                summary: None,
                uri: None,
                mime_type: None,
                payload: serde_json::json!({"ok": true}),
            },
        )
        .await
        .unwrap();
        evidence_store::create_claim(
            &db,
            evidence_store::NewClaimPending {
                id: "claim-1".to_owned(),
                statement: "reads succeed".to_owned(),
                created_by_agent_id: Some("agent-1".to_owned()),
                task_id: Some("task-g1".to_owned()),
            },
        )
        .await
        .unwrap();
        db
    }

    #[tokio::test]
    async fn task_contract_requires_known_task() {
        let db = seeded().await;
        let contract = gather_task_contract(&db, "task-g1").await.unwrap();
        assert_eq!(contract.title, "gather me");
        assert!(gather_task_contract(&db, "missing").await.is_err());
    }

    #[tokio::test]
    async fn evidence_respects_task_scope() {
        let db = seeded().await;
        let pieces = gather_evidence(&db, "task-g1", 50).await.unwrap();
        assert_eq!(pieces.len(), 1);
        assert_eq!(pieces[0].evidence_refs, vec!["ev-1".to_owned()]);
        assert!(gather_evidence(&db, "other", 50).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn memory_comes_from_task_claims() {
        let db = seeded().await;
        let facts = gather_memory(&db, "task-g1", 50).await.unwrap();
        assert_eq!(facts.len(), 1);
        assert!(facts[0].body.contains("reads succeed"));
        assert!(gather_memory(&db, "other", 50).await.unwrap().is_empty());
    }

    #[test]
    fn tool_ranking_returns_relevant_tools() {
        let docs = vec![
            tool_rag::IndexedTool {
                tool_id: "fs.read".to_owned(),
                name: "fs.read".to_owned(),
                description: "read a file from disk".to_owned(),
                capabilities: vec!["ReadFile".to_owned()],
                risk_level: "low".to_owned(),
                reversible: true,
                requires_evidence: false,
                requires_compensation: false,
                enabled: true,
                text: "read file contents from disk".to_owned(),
            },
            tool_rag::IndexedTool {
                tool_id: "process.exec".to_owned(),
                name: "process.exec".to_owned(),
                description: "execute a subprocess".to_owned(),
                capabilities: vec!["ExecuteCommand".to_owned()],
                risk_level: "high".to_owned(),
                reversible: false,
                requires_evidence: true,
                requires_compensation: false,
                enabled: false,
                text: "run shell commands".to_owned(),
            },
        ];
        let ranked = rank_tools(&docs, &HashMap::new(), "read file contents", 5).unwrap();
        assert!(!ranked.is_empty());
        assert_eq!(ranked[0].tool_id, "fs.read");
    }

    #[tokio::test]
    async fn chain_head_tracks_log() {
        let db = seeded().await;
        assert!(chain_head_hash(&db).await.unwrap().is_some());
    }
}
