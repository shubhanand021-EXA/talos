//! Semantic memory IPC: hybrid search over claims, evidence, tasks, skills.
//!
//! Every search first runs an incremental index pass (only new or changed
//! ledger rows are embedded), then blends lexical and semantic signals with
//! recency and verified boosts. Queries never leave the process; events
//! carry counts and refs, never prompt text.

use talos_common::TalosError;

use crate::commands::IpcError;
use crate::events::{emit_talos_event, INDEX_REBUILT, MEMORY_INDEXED, SKILL_CREATED};
use crate::state::AppState;

/// One memory hit shaped for the TS `SearchResult` schema (`camelCase`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemorySearchResultDto {
    /// Entry id (`{source_type}:{source_id}`).
    pub id: String,
    /// `claim` | `evidence` | `task` | `skill`.
    pub source_type: String,
    /// Source row id.
    pub source_id: String,
    /// Owning task, if any.
    pub task_id: Option<String>,
    /// Blended score in [0, 1].
    pub score: f64,
    /// Token-overlap component in [0, 1].
    pub lexical_score: f64,
    /// Cosine-similarity component in [-1, 1].
    pub semantic_score: f64,
    /// Leading text snippet.
    pub snippet: String,
    /// Verified flag from index time.
    pub verified: bool,
    /// Entry creation timestamp.
    pub created_at: String,
    /// Blend weights, boosts, and source ref.
    pub reason: String,
}

impl From<memory_indexer::SearchResult> for MemorySearchResultDto {
    fn from(hit: memory_indexer::SearchResult) -> Self {
        Self {
            id: hit.id,
            source_type: hit.source_type,
            source_id: hit.source_id,
            task_id: hit.task_id,
            score: hit.score,
            lexical_score: hit.lexical_score,
            semantic_score: hit.semantic_score,
            snippet: hit.snippet,
            verified: hit.verified,
            created_at: hit.created_at,
            reason: hit.reason,
        }
    }
}

/// Per-source-type index count.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexSourceCount {
    /// Source type.
    pub source_type: String,
    /// Entry count.
    pub count: i64,
}

/// Index status shaped for the TS `IndexStatus` schema (`camelCase`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexStatusDto {
    /// Total indexed entries.
    pub entries: i64,
    /// Counts grouped by source type.
    pub by_source_type: Vec<IndexSourceCount>,
    /// Embedding backend name.
    pub backend: String,
    /// Embedding width.
    pub dims: usize,
    /// Always false until an ONNX backend ships.
    pub model_loaded: bool,
    /// Probed model directory.
    pub model_dir: String,
    /// Fallback note (empty when a real model backend is active).
    pub warning: String,
    /// Most recent index timestamp, if anything was indexed.
    pub last_indexed_at: Option<String>,
}

/// Skill shaped for the TS `Skill` schema (`camelCase`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillDto {
    /// Skill identifier.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// What the skill does.
    pub description: String,
    /// Skill version.
    pub version: String,
    /// `quarantined` | `validated` | `published`.
    pub status: String,
    /// Task the skill was distilled from, if any.
    pub source_task_id: Option<String>,
    /// Permission ceiling.
    pub permission_ceiling: String,
    /// Required tool ids.
    pub required_tools: Vec<String>,
    /// RFC 3339 creation timestamp.
    pub created_at: String,
}

impl From<evidence_store::SkillRow> for SkillDto {
    fn from(row: evidence_store::SkillRow) -> Self {
        let required_tools: Vec<String> =
            serde_json::from_str(&row.required_tools).unwrap_or_default();
        Self {
            id: row.id,
            name: row.name,
            description: row.description,
            version: row.version,
            status: row.status,
            source_task_id: row.source_task_id,
            permission_ceiling: row.permission_ceiling,
            required_tools,
            created_at: row.created_at,
        }
    }
}

fn check_query(query: &str) -> Result<String, IpcError> {
    let trimmed = query.trim().to_owned();
    if trimmed.is_empty() || trimmed.len() > 1024 {
        return Err(IpcError::from(TalosError::InvalidId {
            what: "query",
            reason: "must be 1..=1024 chars".to_owned(),
        }));
    }
    Ok(trimmed)
}

/// Inner hybrid memory search (shared by the command and tests).
///
/// Runs an incremental index pass first so results always reflect the
/// current ledger, then searches with scope filters applied before scoring.
#[allow(clippy::too_many_arguments)]
pub async fn search_memory_inner(
    state: &AppState,
    app: Option<&tauri::AppHandle>,
    query: String,
    top_k: Option<u8>,
    source_types: Vec<String>,
    task_id: Option<String>,
    verified_only: bool,
    mode: Option<String>,
) -> Result<Vec<MemorySearchResultDto>, IpcError> {
    let query = check_query(&query)?;
    let mode = match mode.as_deref() {
        None | Some("") => memory_indexer::RetrievalMode::Hybrid,
        Some(name) => memory_indexer::RetrievalMode::parse(name).map_err(IpcError::from)?,
    };
    for source_type in &source_types {
        if !vector_store::SOURCE_TYPES.contains(&source_type.as_str()) {
            return Err(IpcError::from(TalosError::InvalidId {
                what: "source_types",
                reason: "must be claim|evidence|task|skill".to_owned(),
            }));
        }
    }
    if let Some(task) = task_id.as_deref() {
        if task.is_empty() || task.len() > 128 {
            return Err(IpcError::from(TalosError::InvalidId {
                what: "task_id",
                reason: "must be 1..=128 chars".to_owned(),
            }));
        }
    }
    // Freshness first: index new/changed rows before searching.
    let report = memory_indexer::index_incremental(&state.db).await?;
    for skill_id in &report.new_skill_ids {
        if let Some(handle) = app {
            let _ = emit_talos_event(
                handle,
                SKILL_CREATED,
                serde_json::json!({
                    "skillId": skill_id,
                    "status": "quarantined",
                    "checkedAt": talos_common::now_iso(),
                }),
            );
        }
    }
    let filter = memory_indexer::MemoryFilter {
        source_types,
        task_id,
        since: None,
        until: None,
        verified_only,
    };
    let alpha = match mode {
        memory_indexer::RetrievalMode::Hybrid => 0.5,
        memory_indexer::RetrievalMode::Semantic => 1.0,
        memory_indexer::RetrievalMode::Lexical => 0.0,
    };
    let hits = memory_indexer::hybrid_search(
        &state.db,
        &query,
        top_k.unwrap_or(10).clamp(1, 50) as usize,
        &filter,
        mode,
        alpha,
    )
    .await?;
    if let Some(handle) = app {
        let _ = emit_talos_event(
            handle,
            MEMORY_INDEXED,
            serde_json::json!({
                "queryChars": query.len(),
                "resultCount": hits.len(),
                "mode": mode.as_str(),
                "checkedAt": talos_common::now_iso(),
            }),
        );
    }
    Ok(hits.into_iter().map(MemorySearchResultDto::from).collect())
}

/// Inner index status (shared by the command and tests).
pub async fn get_index_status_inner(state: &AppState) -> Result<IndexStatusDto, IpcError> {
    let entries = vector_store::count(&state.db).await?;
    let by_source_type = vector_store::counts_by_type(&state.db)
        .await?
        .into_iter()
        .map(|(source_type, count)| IndexSourceCount { source_type, count })
        .collect();
    let model = embedding_engine::model_dir_status();
    let last_indexed_at = vector_store::last_indexed_at(&state.db).await?;
    Ok(IndexStatusDto {
        entries,
        by_source_type,
        backend: model.backend.to_owned(),
        dims: embedding_engine::EMBEDDING_DIMS,
        model_loaded: model.model_loaded,
        model_dir: model.dir,
        warning: model.warning,
        last_indexed_at,
    })
}

/// Inner full rebuild: clears vectors + state, then reindexes everything.
pub async fn rebuild_index_inner(
    state: &AppState,
    app: Option<&tauri::AppHandle>,
) -> Result<IndexStatusDto, IpcError> {
    vector_store::clear_all(&state.db).await?;
    let report = memory_indexer::index_incremental(&state.db).await?;
    for skill_id in &report.new_skill_ids {
        if let Some(handle) = app {
            let _ = emit_talos_event(
                handle,
                SKILL_CREATED,
                serde_json::json!({
                    "skillId": skill_id,
                    "status": "quarantined",
                    "checkedAt": talos_common::now_iso(),
                }),
            );
        }
    }
    if let Some(handle) = app {
        let _ = emit_talos_event(
            handle,
            INDEX_REBUILT,
            serde_json::json!({
                "entriesAdded": report.entries_added,
                "checkedAt": talos_common::now_iso(),
            }),
        );
    }
    get_index_status_inner(state).await
}

/// Inner skill listing (shared by the command and tests).
pub async fn list_skills_inner(
    state: &AppState,
    status: Option<String>,
) -> Result<Vec<SkillDto>, IpcError> {
    if let Some(wanted) = status.as_deref() {
        if !evidence_store::SKILL_STATUSES.contains(&wanted) {
            return Err(IpcError::from(TalosError::InvalidId {
                what: "status",
                reason: "must be quarantined|validated|published".to_owned(),
            }));
        }
    }
    Ok(
        evidence_store::list_skills(&state.db, status.as_deref(), 200)
            .await
            .map_err(IpcError::from)?
            .into_iter()
            .map(SkillDto::from)
            .collect(),
    )
}

/// `search_memory` — incremental index, then hybrid recall over the ledger.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn search_memory(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    query: String,
    top_k: Option<u8>,
    source_types: Option<Vec<String>>,
    task_id: Option<String>,
    verified_only: Option<bool>,
    mode: Option<String>,
) -> Result<Vec<MemorySearchResultDto>, IpcError> {
    search_memory_inner(
        &state,
        Some(&app),
        query,
        top_k,
        source_types.unwrap_or_default(),
        task_id,
        verified_only.unwrap_or(false),
        mode,
    )
    .await
}

/// `get_index_status` — vector counts, backend, model probe, last rebuild.
#[tauri::command]
pub async fn get_index_status(
    state: tauri::State<'_, AppState>,
) -> Result<IndexStatusDto, IpcError> {
    get_index_status_inner(&state).await
}

/// `rebuild_index` — clear vectors + state, then reindex from the ledger.
#[tauri::command]
pub async fn rebuild_index(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<IndexStatusDto, IpcError> {
    rebuild_index_inner(&state, Some(&app)).await
}

/// `list_skills` — skill registry (optional status filter).
#[tauri::command]
pub async fn list_skills(
    state: tauri::State<'_, AppState>,
    status: Option<String>,
) -> Result<Vec<SkillDto>, IpcError> {
    list_skills_inner(&state, status).await
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_state() -> AppState {
        AppState::new_in_memory()
            .await
            .expect("in-memory state builds")
    }

    async fn seed_memory(state: &AppState) {
        let mut seq = {
            let mut runtime = state.runtime.lock().await;
            runtime.next_queue_seq()
        };
        agent_runtime::queue::submit_task(
            &state.db,
            &mut seq,
            agent_runtime::queue::SubmitParams {
                title: "read then hash the manifest".to_owned(),
                description: Some("verify the ledger after reads".to_owned()),
                priority: "normal".to_owned(),
                timeout_ms: 5_000,
                max_attempts: 2,
                scenario: "quick_success".to_owned(),
                parent_task_id: None,
            },
        )
        .await
        .expect("task");
        evidence_store::insert_evidence(
            &state.db,
            evidence_store::NewEvidence {
                id: "ev-mem-1".to_owned(),
                task_id: None,
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
        evidence_store::transition_evidence_status(&state.db, "ev-mem-1", "verified")
            .await
            .expect("verify");
        evidence_store::create_claim(
            &state.db,
            evidence_store::NewClaimPending {
                id: "claim-mem-1".to_owned(),
                statement: "reads succeed with hash".to_owned(),
                created_by_agent_id: None,
                task_id: None,
            },
        )
        .await
        .expect("claim");
    }

    #[tokio::test]
    async fn search_finds_indexed_memory() {
        let state = test_state().await;
        seed_memory(&state).await;
        let hits = search_memory_inner(
            &state,
            None,
            "read manifest hash".to_owned(),
            Some(10),
            Vec::new(),
            None,
            false,
            None,
        )
        .await
        .expect("search");
        assert!(!hits.is_empty());
        assert!(hits[0].score > 0.0);
        assert!(!hits[0].reason.is_empty());
    }

    #[tokio::test]
    async fn scope_filters_and_modes_work() {
        let state = test_state().await;
        seed_memory(&state).await;
        // Unknown task scope finds nothing.
        let scoped = search_memory_inner(
            &state,
            None,
            "reads succeed".to_owned(),
            Some(10),
            Vec::new(),
            Some("task-nope".to_owned()),
            false,
            None,
        )
        .await
        .expect("search");
        assert!(scoped.is_empty());
        // Lexical-only still recalls the claim verbatim.
        let lexical = search_memory_inner(
            &state,
            None,
            "reads succeed".to_owned(),
            Some(10),
            Vec::new(),
            None,
            false,
            Some("lexical".to_owned()),
        )
        .await
        .expect("search");
        assert!(!lexical.is_empty());
        // Semantic-only recalls without keyword overlap.
        let semantic = search_memory_inner(
            &state,
            None,
            "read manifest hash".to_owned(),
            Some(10),
            Vec::new(),
            None,
            false,
            Some("semantic".to_owned()),
        )
        .await
        .expect("search");
        assert!(!semantic.is_empty());
        // Bad mode and bad query are transport errors.
        assert!(search_memory_inner(
            &state,
            None,
            "reads".to_owned(),
            Some(10),
            Vec::new(),
            None,
            false,
            Some("bogus".to_owned())
        )
        .await
        .is_err());
        assert!(search_memory_inner(
            &state,
            None,
            "   ".to_owned(),
            Some(10),
            Vec::new(),
            None,
            false,
            None
        )
        .await
        .is_err());
        assert!(search_memory_inner(
            &state,
            None,
            "reads".to_owned(),
            Some(10),
            vec!["weird".to_owned()],
            None,
            false,
            None
        )
        .await
        .is_err());
    }

    #[tokio::test]
    async fn status_and_rebuild_round_trip() {
        let state = test_state().await;
        seed_memory(&state).await;
        let empty = get_index_status_inner(&state).await.expect("status");
        assert_eq!(empty.entries, 0);
        assert!(!empty.backend.is_empty());
        assert!(!empty.model_loaded);
        let rebuilt = rebuild_index_inner(&state, None).await.expect("rebuild");
        assert!(rebuilt.entries >= 2);
        assert!(rebuilt.last_indexed_at.is_some());
        let listed = list_skills_inner(&state, None).await.expect("skills");
        // No completed tasks with verified claims: no quarantined skills yet.
        assert!(listed.is_empty());
        assert!(list_skills_inner(&state, Some("bogus".to_owned()))
            .await
            .is_err());
    }
}
