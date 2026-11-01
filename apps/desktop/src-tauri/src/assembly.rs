//! Context Assembly Inspector backend: preview, config get/update.
//!
//! `preview_context` gathers task-scoped context (ledger, Tool-RAG, session),
//! assembles the budgeted prompt, persists `context_assembly` metadata as
//! verified evidence, and returns structure/counts/hashes — never prompt
//! text. Over-budget assemblies fail with `context_budget_exceeded` and emit
//! `contextBudgetExceeded`.

use talos_common::TalosError;

use crate::commands::IpcError;
use crate::events::{emit_talos_event, CONTEXT_BUDGET_EXCEEDED};
use crate::state::AppState;

/// Verbatim safety rules stamping every assembly (never compressed).
pub const SAFETY_RULES: &[&str] = &[
    "Default deny: no action executes without an explicit allow rule.",
    "Write-like actions require recorded evidence first.",
    "Irreversible actions require a compensation plan.",
    "High-risk non-reversible actions require human review.",
];

/// Compaction applied during one assembly (mirrors the TS schema).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssemblyCompactionDto {
    /// Layer name.
    pub layer: String,
    /// Tokens before.
    pub original_tokens: u64,
    /// Tokens after.
    pub compacted_tokens: u64,
    /// TypeCompact proof hash (`truncated` when degraded).
    pub proof_hash: String,
    /// Invariants the compact step preserved (empty when degraded).
    pub preserved_invariants: Vec<String>,
    /// True when truncation replaced compaction.
    pub degraded: bool,
}

/// Per-layer accounting for one assembly.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssembledLayerDto {
    /// Layer name.
    pub layer: String,
    /// Tokens used.
    pub used_tokens: u32,
    /// Budget target.
    pub target_tokens: u32,
    /// True when the layer was compacted.
    pub compacted: bool,
}

/// Assembly metadata shaped like the TS schema (no prompt text).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssembledContextMetadataDto {
    /// Task id.
    pub task_id: String,
    /// Model id.
    pub model: String,
    /// Model context window.
    pub context_window: u32,
    /// Total prompt tokens.
    pub total_tokens: u32,
    /// Per-layer accounting.
    pub layers: Vec<AssembledLayerDto>,
    /// Tool ids in the catalog block.
    pub tools_included: Vec<String>,
    /// Evidence ids referenced.
    pub evidence_refs: Vec<String>,
    /// Template version.
    pub template_version: String,
    /// Estimator identity.
    pub estimator_used: String,
    /// SHA-256 of the canonical prompt (audit).
    pub assembly_hash: String,
    /// Compactions applied.
    pub compactions_applied: Vec<AssemblyCompactionDto>,
    /// True when any layer fell back to truncation.
    pub degraded: bool,
    /// Assembly timestamp.
    pub executed_at: String,
}

/// Configuration snapshot shaped like the TS schema.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextAssemblyConfigDto {
    /// Bucket percentages (sum to 100).
    pub allocation: context_assembler::AssemblyAllocation,
    /// Output reservation percent.
    pub reserved_output_pct: u8,
    /// Template version.
    pub template_version: String,
    /// Fallback window.
    pub default_context_window: u32,
}

impl From<context_assembler::ContextAssemblyConfig> for ContextAssemblyConfigDto {
    fn from(config: context_assembler::ContextAssemblyConfig) -> Self {
        Self {
            allocation: config.allocation,
            reserved_output_pct: config.reserved_output_pct,
            template_version: config.template_version,
            default_context_window: config.default_context_window,
        }
    }
}

fn valid_task_id(task_id: &str) -> Result<(), IpcError> {
    if task_id.is_empty() || task_id.len() > 128 {
        return Err(IpcError::from(TalosError::InvalidId {
            what: "task_id",
            reason: "must be 1..=128 chars".to_owned(),
        }));
    }
    Ok(())
}

/// `preview_context` — gather, assemble, record, and return metadata.
/// Prompt text never crosses IPC; only structure, counts, and hashes.
pub async fn preview_context_inner(
    state: &AppState,
    app: Option<&tauri::AppHandle>,
    task_id: String,
    model: Option<String>,
    intent: Option<String>,
    instruction: Option<String>,
    context_window: Option<u32>,
) -> Result<AssembledContextMetadataDto, IpcError> {
    valid_task_id(&task_id)?;
    let model = model.unwrap_or_else(|| "gpt-4o-mini".to_owned());
    if model.is_empty() || model.len() > 128 {
        return Err(IpcError::from(TalosError::InvalidId {
            what: "model",
            reason: "must be 1..=128 chars".to_owned(),
        }));
    }

    // Task contract (unknown tasks are an error: no anonymous assembly).
    let contract = context_assembler::gather_task_contract(&state.db, &task_id).await?;
    let evidence = context_assembler::gather_evidence(&state.db, &task_id, 50).await?;
    let memory = context_assembler::gather_memory(&state.db, &task_id, 20).await?;
    let session = context_assembler::gather_session(&state.db, &task_id, 20).await?;
    let chain_head = context_assembler::chain_head_hash(&state.db).await?;

    // Tool catalog: rank the registry index against the task intent.
    let registry = state.tools.lock().await;
    let manifests = registry.list();
    let enabled: std::collections::HashMap<String, bool> = manifests
        .iter()
        .map(|manifest| (manifest.id.clone(), registry.is_enabled(&manifest.id)))
        .collect();
    drop(registry);
    let evidence_rows = evidence_store::list_recent_evidence(&state.db, 200).await?;
    let tasks = evidence_store::list_tasks(&state.db, 200).await?;
    let plans = evidence_store::list_compensation_plans(&state.db, 50).await?;
    let docs = tool_rag::build_indexed_tools(
        &manifests,
        &tool_rag::receipt_tool_tasks(&evidence_rows),
        &tasks,
        &tool_rag::evidence_kind_list(&evidence_rows),
        &tool_rag::plan_strategy_list(&plans),
        &enabled,
    );
    let stats = tool_rag::stats_from_evidence(&evidence_rows);
    let query_intent = intent.unwrap_or_else(|| contract.title.clone());
    let ranked = context_assembler::rank_tools(&docs, &stats, &query_intent, 8)
        .map_err(|err| IpcError::from(TalosError::Storage(err)))?;
    let by_id: std::collections::HashMap<&str, &tool_runtime::ToolManifest> = manifests
        .iter()
        .map(|manifest| (manifest.id.as_str(), manifest))
        .collect();
    let tool_entries: Vec<context_assembler::ToolEntry> = ranked
        .iter()
        .filter_map(|tool| by_id.get(tool.tool_id.as_str()))
        .map(|manifest| context_assembler::ToolEntry {
            name: manifest.id.clone(),
            description: manifest.description.clone(),
            input_schema: manifest.input_schema.clone(),
        })
        .collect();

    let mut segments = Vec::new();
    segments.extend(evidence);
    segments.extend(memory);
    segments.extend(session);

    let gathered = context_assembler::GatheredContext {
        task_id: task_id.clone(),
        agent_id: "agent-local".to_owned(),
        session_id: "session-local".to_owned(),
        goal: contract.title.clone(),
        acceptance: Vec::new(),
        constraints: vec![
            format!("priority: {}", contract.priority),
            format!("status: {}", contract.status),
        ],
        task_title: contract.title.clone(),
        task_status: contract.status.clone(),
        task_priority: contract.priority.clone(),
        safety_rules: SAFETY_RULES.iter().map(|rule| (*rule).to_owned()).collect(),
        segments,
        ranked_tools: ranked
            .into_iter()
            .map(|tool| context_assembler::RankedTool {
                tool_id: tool.tool_id,
                score: tool.score,
                reason: tool.reason,
            })
            .collect(),
        tool_entries,
        history: Vec::new(),
        instruction: instruction.unwrap_or_else(|| "advance the task one step".to_owned()),
        chain_head_hash: chain_head,
    };

    let config = state.context_config.lock().await.clone();
    let window = context_window.unwrap_or(config.default_context_window);
    let assembled = match context_assembler::assemble(&context_assembler::AssemblyInput {
        gathered,
        model: model.clone(),
        context_window: window,
        config,
    }) {
        Ok(assembled) => assembled,
        Err(context_assembler::AssemblyError::HardLimitExceeded { total, window }) => {
            if let Some(handle) = app {
                let _ = emit_talos_event(
                    handle,
                    CONTEXT_BUDGET_EXCEEDED,
                    serde_json::json!({
                        "taskId": task_id,
                        "totalTokens": total,
                        "contextWindow": window,
                        "checkedAt": talos_common::now_iso(),
                    }),
                );
            }
            return Err(IpcError {
                code: "context_budget_exceeded".to_owned(),
                message: format!("context budget exceeded: {total} tokens over {window} window"),
            });
        }
        Err(err) => {
            return Err(IpcError::from(TalosError::Storage(err.to_string())));
        }
    };

    // Record assembly metadata as verified evidence (metadata only).
    let payload = context_assembler::assembly_evidence_payload(&assembled, &task_id, "agent-local");
    let evidence_id = format!(
        "context-assembly-{}-{}",
        task_id,
        assembled.assembly_hash[7..15].to_owned()
    );
    if evidence_store::insert_evidence(
        &state.db,
        evidence_store::NewEvidence {
            id: evidence_id.clone(),
            task_id: Some(task_id.clone()),
            agent_id: Some("agent-local".to_owned()),
            session_id: Some("session-local".to_owned()),
            kind: "context_assembly".to_owned(),
            label: Some(format!("assembly {model}")),
            summary: Some(format!(
                "{} tokens, {} tools, {} refs",
                assembled.total_tokens,
                assembled.tools_included.len(),
                assembled.evidence_refs.len()
            )),
            uri: None,
            mime_type: None,
            payload,
        },
    )
    .await
    .is_ok()
    {
        let _ =
            evidence_store::transition_evidence_status(&state.db, &evidence_id, "verified").await;
    }

    let compacted_layers: std::collections::HashSet<&str> = assembled
        .compactions_applied
        .iter()
        .map(|summary| summary.layer.as_str())
        .collect();
    let mut layers: Vec<AssembledLayerDto> = assembled
        .per_layer_tokens
        .iter()
        .map(|(layer, used)| AssembledLayerDto {
            layer: layer.clone(),
            used_tokens: *used,
            target_tokens: assembled.layer_targets.get(layer).copied().unwrap_or(0),
            compacted: compacted_layers.contains(layer.as_str()),
        })
        .collect();
    layers.sort_by(|a, b| a.layer.cmp(&b.layer));
    Ok(AssembledContextMetadataDto {
        task_id,
        model,
        context_window: window,
        total_tokens: assembled.total_tokens,
        layers,
        tools_included: assembled.tools_included,
        evidence_refs: assembled.evidence_refs,
        template_version: assembled.template_version,
        estimator_used: assembled.estimator_used,
        assembly_hash: assembled.assembly_hash,
        compactions_applied: assembled
            .compactions_applied
            .into_iter()
            .map(|summary| AssemblyCompactionDto {
                layer: summary.layer,
                original_tokens: summary.original_tokens,
                compacted_tokens: summary.compacted_tokens,
                proof_hash: summary.proof_hash,
                preserved_invariants: summary.preserved_invariants,
                degraded: summary.degraded,
            })
            .collect(),
        degraded: assembled.degraded,
        executed_at: talos_common::now_iso(),
    })
}

/// `get_context_config` — current allocation settings.
pub async fn get_context_config_inner(state: &AppState) -> ContextAssemblyConfigDto {
    state.context_config.lock().await.clone().into()
}

/// `update_context_config` — validated allocation update (must sum to 100%).
pub async fn update_context_config_inner(
    state: &AppState,
    allocation: context_assembler::AssemblyAllocation,
    reserved_output_pct: u8,
) -> Result<ContextAssemblyConfigDto, IpcError> {
    let mut config = state.context_config.lock().await;
    let updated = context_assembler::ContextAssemblyConfig {
        allocation,
        reserved_output_pct,
        template_version: config.template_version.clone(),
        default_context_window: config.default_context_window,
    };
    updated
        .validate()
        .map_err(|reason| IpcError::from(TalosError::Storage(reason)))?;
    *config = updated.clone();
    Ok(ContextAssemblyConfigDto::from(updated))
}

/// `preview_context` — assemble and return metadata (no prompt text).
#[tauri::command]
pub async fn preview_context(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    task_id: String,
    model: Option<String>,
    intent: Option<String>,
    instruction: Option<String>,
    context_window: Option<u32>,
) -> Result<AssembledContextMetadataDto, IpcError> {
    preview_context_inner(
        &state,
        Some(&app),
        task_id,
        model,
        intent,
        instruction,
        context_window,
    )
    .await
}

/// `get_context_config` — current allocation settings.
#[tauri::command]
pub async fn get_context_config(
    state: tauri::State<'_, AppState>,
) -> Result<ContextAssemblyConfigDto, IpcError> {
    Ok(get_context_config_inner(&state).await)
}

/// `update_context_config` — validated allocation update.
#[tauri::command]
pub async fn update_context_config(
    state: tauri::State<'_, AppState>,
    allocation: context_assembler::AssemblyAllocation,
    reserved_output_pct: u8,
) -> Result<ContextAssemblyConfigDto, IpcError> {
    update_context_config_inner(&state, allocation, reserved_output_pct).await
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_state() -> AppState {
        AppState::new_in_memory_with_vault(talos_secrets::SecretVault::mock())
            .await
            .expect("mock-vault state builds")
    }

    async fn seeded_task_id() -> (AppState, String) {
        let state = test_state().await;
        let mut seq = 1u64;
        let view = agent_runtime::queue::submit_task(
            &state.db,
            &mut seq,
            agent_runtime::queue::SubmitParams {
                title: "assemble me".to_owned(),
                description: None,
                priority: "normal".to_owned(),
                timeout_ms: 5000,
                max_attempts: 3,
                scenario: "quick_success".to_owned(),
                parent_task_id: None,
            },
        )
        .await
        .expect("task submits");
        (state, view.id)
    }

    #[tokio::test]
    async fn preview_returns_metadata_without_prompt_text() {
        let (state, task_id) = seeded_task_id().await;
        let metadata = preview_context_inner(&state, None, task_id, None, None, None, None)
            .await
            .unwrap();
        assert!(metadata.total_tokens > 0);
        assert_eq!(metadata.template_version, "ctx-assembler/1");
        assert!(metadata.assembly_hash.starts_with("sha256:"));
        assert!(metadata.layers.iter().any(|layer| layer.layer == "safety"));
        assert!(!metadata.degraded);
        // Metadata only: evidence refs and tools, no message bodies.
        let raw = serde_json::to_string(&metadata).unwrap();
        assert!(!raw.contains("advance the task one step"));
    }

    #[tokio::test]
    async fn preview_rejects_unknown_tasks() {
        let state = test_state().await;
        let err = preview_context_inner(&state, None, "missing".to_owned(), None, None, None, None)
            .await
            .unwrap_err();
        assert!(!err.message.is_empty());
    }

    #[tokio::test]
    async fn preview_fails_gracefully_over_hard_limit() {
        let (state, task_id) = seeded_task_id().await;
        let err = preview_context_inner(
            &state,
            None,
            task_id,
            Some("llama3".to_owned()),
            None,
            None,
            Some(32),
        )
        .await
        .unwrap_err();
        assert_eq!(err.code, "context_budget_exceeded");
    }

    #[tokio::test]
    async fn config_round_trip_validates_sums() {
        let state = test_state().await;
        let current = get_context_config_inner(&state).await;
        assert_eq!(current.template_version, "ctx-assembler/1");
        let updated = update_context_config_inner(
            &state,
            context_assembler::AssemblyAllocation {
                contracts_pct: 25,
                tool_state_pct: 25,
                evidence_pct: 30,
                memory_pct: 10,
                plan_pct: 10,
            },
            15,
        )
        .await
        .unwrap();
        assert_eq!(updated.allocation.evidence_pct, 30);
        let bad = update_context_config_inner(
            &state,
            context_assembler::AssemblyAllocation {
                contracts_pct: 25,
                tool_state_pct: 25,
                evidence_pct: 30,
                memory_pct: 10,
                plan_pct: 11,
            },
            15,
        )
        .await;
        assert!(bad.is_err());
    }
}
