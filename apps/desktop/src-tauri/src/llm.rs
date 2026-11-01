//! BYOK provider commands: status/catalog/probe/keys/usage.
//!
//! Secret values never cross this boundary outward: `store_provider_key`
//! writes into the OS keychain and returns `true`; `test_provider_connection`
//! reports success/latency/message only. Every live probe runs through the
//! policy-gated [`llm_gateway`] and emits request lifecycle events.

use talos_common::TalosError;

use crate::commands::IpcError;
use crate::events::{
    emit_talos_event, LLM_BUDGET_WARNING, LLM_REQUEST_COMPLETED, LLM_REQUEST_STARTED,
};
use crate::state::AppState;

/// Provider status shaped like the TS `ProviderStatus` schema.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatusDto {
    /// Provider id (`openai`, `anthropic`, `ollama`).
    pub provider_id: String,
    /// True when a key is stored (Ollama is keyless: always true).
    pub configured: bool,
    /// Last probe result (None = never probed this session).
    pub reachable: Option<bool>,
    /// Last probe latency in milliseconds, if probed.
    pub latency_ms: Option<u64>,
    /// Models from the gateway catalog.
    pub models: Vec<String>,
}

/// Probe result shaped for the TS bridge (`ok` + message, never a key).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionTestDto {
    /// Provider id.
    pub provider_id: String,
    /// True when the probe completion succeeded.
    pub ok: bool,
    /// Round-trip latency in milliseconds, if served.
    pub latency_ms: Option<u64>,
    /// Human-readable outcome (safe: gateway errors never carry secrets).
    pub message: String,
}

/// Per-provider usage slice for the TS `UsageSummary` schema.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUsageDto {
    /// Provider id.
    pub provider_id: String,
    /// Summed tokens.
    pub total_tokens: i64,
    /// Summed estimated cost in USD.
    pub estimated_cost_usd: f64,
    /// Recorded calls.
    pub call_count: i64,
}

/// Session usage shaped like the TS `UsageSummary` schema.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSummaryDto {
    /// Summed tokens across providers.
    pub total_tokens: i64,
    /// Summed estimated cost in USD.
    pub estimated_cost_usd: f64,
    /// Recorded calls.
    pub call_count: i64,
    /// Per-provider breakdown.
    pub by_provider: Vec<ProviderUsageDto>,
}

fn valid_provider_id(provider_id: &str) -> Result<(), IpcError> {
    if provider_id.is_empty() || provider_id.len() > 64 {
        return Err(IpcError::from(TalosError::InvalidId {
            what: "provider_id",
            reason: "must be 1..=64 chars".to_owned(),
        }));
    }
    Ok(())
}

/// `list_providers` — status + catalog model ids per provider. The keychain
/// is consulted for presence only; values never leave the vault.
pub async fn list_providers_inner(state: &AppState) -> Result<Vec<ProviderStatusDto>, IpcError> {
    let gateway = state.llm.lock().await;
    let mut out = Vec::new();
    for (provider_id, configured) in gateway.provider_statuses() {
        let models: Vec<String> = gateway
            .models_for(&provider_id)
            .await
            .into_iter()
            .map(|d| d.model_id)
            .collect();
        out.push(ProviderStatusDto {
            provider_id,
            configured,
            reachable: None,
            latency_ms: None,
            models,
        });
    }
    Ok(out)
}

/// `list_models` — gateway catalog descriptors for one provider.
pub async fn list_models_inner(
    state: &AppState,
    provider_id: String,
) -> Result<Vec<llm_gateway::ModelDescriptor>, IpcError> {
    valid_provider_id(&provider_id)?;
    let gateway = state.llm.lock().await;
    Ok(gateway.models_for(&provider_id).await)
}

/// Shared gated completion with lifecycle events (probe path included).
/// Emits `llmRequestStarted`/`llmRequestCompleted` plus `llmBudgetWarning`
/// once a capped task crosses 80% of its spend.
pub async fn complete_with_events(
    state: &AppState,
    app: Option<&tauri::AppHandle>,
    provider_id: &str,
    request: llm_gateway::LlmRequest,
) -> Result<(llm_gateway::LlmResponse, llm_gateway::ProviderReceipt), IpcError> {
    if let Some(handle) = app {
        let _ = emit_talos_event(
            handle,
            LLM_REQUEST_STARTED,
            serde_json::json!({
                "taskId": request.metadata.task_id,
                "agentId": request.metadata.agent_id,
                "providerId": provider_id,
                "model": request.model,
                "checkedAt": talos_common::now_iso(),
            }),
        );
    }
    let gateway = state.llm.lock().await;
    // Session budget gate: deny new model calls once global spend exceeds
    // the configured cap (uncapped sessions always pass).
    crate::telemetry::check_session_budget(state)
        .await
        .map_err(|err| IpcError {
            code: err.code,
            message: err.message,
        })?;
    let outcome = gateway
        .complete(request.clone(), None)
        .await
        .map_err(|err| IpcError {
            code: llm_error_code(&err),
            message: err.to_string(),
        });
    drop(gateway);
    let (response, receipt) = outcome?;
    if let Some(handle) = app {
        let _ = emit_talos_event(
            handle,
            LLM_REQUEST_COMPLETED,
            serde_json::json!({
                "taskId": request.metadata.task_id,
                "providerId": provider_id,
                "model": receipt.model_id,
                "totalTokens": receipt.usage.total_tokens,
                "estimatedCostUsd": receipt.estimated_cost_usd,
                "latencyMs": receipt.latency_ms,
                "checkedAt": talos_common::now_iso(),
            }),
        );
        emit_budget_warning_if_needed(state, handle, &request.metadata.task_id).await;
    }
    Ok((response, receipt))
}

/// `test_provider_connection` — one minimal gated completion. Returns
/// success/latency/message; the key is never returned.
pub async fn test_provider_connection_inner(
    state: &AppState,
    app: Option<&tauri::AppHandle>,
    provider_id: String,
    model: Option<String>,
) -> Result<ConnectionTestDto, IpcError> {
    valid_provider_id(&provider_id)?;
    if let Some(explicit) = &model {
        if explicit.is_empty() || explicit.len() > 128 {
            return Err(IpcError::from(TalosError::InvalidId {
                what: "model",
                reason: "must be 1..=128 chars".to_owned(),
            }));
        }
    }
    let chosen = match model {
        Some(explicit) => explicit,
        None => {
            let gateway = state.llm.lock().await;
            let first = gateway.models_for(&provider_id).await.into_iter().next();
            drop(gateway);
            match first {
                Some(descriptor) => descriptor.model_id,
                None => {
                    let message = format!("no models known for provider {provider_id}");
                    return Ok(ConnectionTestDto {
                        provider_id,
                        ok: false,
                        latency_ms: None,
                        message,
                    });
                }
            }
        }
    };
    let request = llm_gateway::LlmRequest {
        model: chosen,
        messages: vec![llm_gateway::LlmMessage::new(
            llm_gateway::LlmRole::User,
            "Say hello in five words or fewer.",
        )],
        tools: None,
        temperature: Some(0.0),
        max_tokens: Some(32),
        stop_sequences: Vec::new(),
        metadata: llm_gateway::LlmRequestMetadata {
            task_id: "connection-probe".to_owned(),
            agent_id: "connection-probe".to_owned(),
            purpose: "provider connectivity probe".to_owned(),
        },
    };
    let started = std::time::Instant::now();
    match complete_with_events(state, app, &provider_id, request).await {
        Ok((_, receipt)) => Ok(ConnectionTestDto {
            provider_id,
            ok: true,
            latency_ms: Some(started.elapsed().as_millis() as u64),
            message: format!(
                "served by {} in {} ms ({} tokens)",
                receipt.model_id, receipt.latency_ms, receipt.usage.total_tokens
            ),
        }),
        Err(ipc) => Ok(ConnectionTestDto {
            provider_id,
            ok: false,
            latency_ms: None,
            message: ipc.message,
        }),
    }
}

async fn emit_budget_warning_if_needed(state: &AppState, handle: &tauri::AppHandle, task_id: &str) {
    let Ok(cap) = evidence_store::get_task_budget(&state.db, task_id).await else {
        return;
    };
    let Some(cap) = cap else { return };
    if cap <= 0.0 {
        return;
    }
    let Ok(spent) = evidence_store::sum_task_spend(&state.db, task_id).await else {
        return;
    };
    if spent >= 0.8 * cap {
        let _ = emit_talos_event(
            handle,
            LLM_BUDGET_WARNING,
            serde_json::json!({
                "taskId": task_id,
                "spentUsd": spent,
                "budgetUsd": cap,
                "checkedAt": talos_common::now_iso(),
            }),
        );
    }
}

fn llm_error_code(err: &llm_gateway::LlmError) -> String {
    use llm_gateway::LlmError as E;
    match err {
        E::InvalidRequest(_) | E::ModelUnavailable(_) => "invalid_request",
        E::AuthFailure | E::SecretUnavailable => "secret_unavailable",
        E::RateLimited => "rate_limited",
        E::Timeout => "timeout",
        E::BudgetExceeded => "budget_exceeded",
        E::PolicyDenied(_) => "policy_denied",
        E::Transient(_) | E::Provider(_) | E::Storage(_) => "provider_unavailable",
    }
    .to_owned()
}

/// `get_usage_summary` — session totals from the append-only usage ledger.
pub async fn get_usage_summary_inner(state: &AppState) -> Result<UsageSummaryDto, IpcError> {
    let (total_tokens, estimated_cost_usd, call_count) =
        evidence_store::summarize_usage(&state.db).await?;
    let by_provider = evidence_store::summarize_usage_by_provider(&state.db)
        .await?
        .into_iter()
        .map(
            |(provider_id, total_tokens, estimated_cost_usd, call_count)| ProviderUsageDto {
                provider_id,
                total_tokens,
                estimated_cost_usd,
                call_count,
            },
        )
        .collect();
    Ok(UsageSummaryDto {
        total_tokens,
        estimated_cost_usd,
        call_count,
        by_provider,
    })
}

/// `store_provider_key` — persist a key in the OS keychain. The value is
/// never echoed back; success is a bare boolean.
pub async fn store_provider_key_inner(
    state: &AppState,
    provider_id: String,
    api_key: String,
) -> Result<bool, IpcError> {
    valid_provider_id(&provider_id)?;
    if api_key.is_empty() || api_key.len() > 4096 {
        return Err(IpcError::from(TalosError::InvalidId {
            what: "api_key",
            reason: "must be 1..=4096 chars".to_owned(),
        }));
    }
    let gateway = state.llm.lock().await;
    let known = gateway
        .provider_statuses()
        .into_iter()
        .any(|(id, _)| id == provider_id)
        || provider_id == "custom";
    drop(gateway);
    if !known {
        return Err(IpcError::from(TalosError::Storage(format!(
            "unknown provider: {provider_id}"
        ))));
    }
    state.secrets.store_secret(
        talos_secrets::TALOS_SERVICE,
        &talos_secrets::provider_key_name(&provider_id),
        &api_key,
    )?;
    Ok(true)
}

/// `delete_provider_key` — remove a key from the OS keychain (idempotent).
pub async fn delete_provider_key_inner(
    state: &AppState,
    provider_id: String,
) -> Result<bool, IpcError> {
    valid_provider_id(&provider_id)?;
    state.secrets.delete_secret(
        talos_secrets::TALOS_SERVICE,
        &talos_secrets::provider_key_name(&provider_id),
    )?;
    Ok(true)
}

/// `list_providers` — provider ids, key presence, and catalog models.
#[tauri::command]
pub async fn list_providers(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<ProviderStatusDto>, IpcError> {
    list_providers_inner(&state).await
}

/// `list_models` — catalog descriptors for one provider.
#[tauri::command]
pub async fn list_models(
    state: tauri::State<'_, AppState>,
    provider_id: String,
) -> Result<Vec<llm_gateway::ModelDescriptor>, IpcError> {
    list_models_inner(&state, provider_id).await
}

/// `test_provider_connection` — gated probe completion; never returns a key.
#[tauri::command]
pub async fn test_provider_connection(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    provider_id: String,
    model: Option<String>,
) -> Result<ConnectionTestDto, IpcError> {
    test_provider_connection_inner(&state, Some(&app), provider_id, model).await
}

/// `get_usage_summary` — cumulative tokens/cost from the usage ledger.
#[tauri::command]
pub async fn get_usage_summary(
    state: tauri::State<'_, AppState>,
) -> Result<UsageSummaryDto, IpcError> {
    get_usage_summary_inner(&state).await
}

/// `store_provider_key` — write a key to the OS keychain; returns `true`.
/// The key value is never echoed back over IPC.
#[tauri::command]
pub async fn store_provider_key(
    state: tauri::State<'_, AppState>,
    provider_id: String,
    api_key: String,
) -> Result<bool, IpcError> {
    store_provider_key_inner(&state, provider_id, api_key).await
}

/// `delete_provider_key` — remove a key from the OS keychain.
#[tauri::command]
pub async fn delete_provider_key(
    state: tauri::State<'_, AppState>,
    provider_id: String,
) -> Result<bool, IpcError> {
    delete_provider_key_inner(&state, provider_id).await
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_state() -> AppState {
        AppState::new_in_memory_with_vault(talos_secrets::SecretVault::mock())
            .await
            .expect("mock-vault state builds")
    }

    #[tokio::test]
    async fn providers_list_with_presence_flags() {
        let state = test_state().await;
        let providers = list_providers_inner(&state).await.unwrap();
        let ids: Vec<&str> = providers.iter().map(|p| p.provider_id.as_str()).collect();
        assert_eq!(ids, vec!["anthropic", "ollama", "openai"]);
        for provider in &providers {
            assert_eq!(provider.reachable, None);
            assert!(!provider.models.is_empty() || provider.provider_id == "custom");
        }
        let openai = providers
            .iter()
            .find(|p| p.provider_id == "openai")
            .unwrap();
        assert!(!openai.configured);
    }

    #[tokio::test]
    async fn provider_key_roundtrip_never_echoes() {
        // Mock vault: deterministic store/get/delete/list with no OS I/O.
        let state = test_state().await;
        let key_name = "provider.testprobe.api_key";
        state
            .secrets
            .store_secret("com.talos.desktop", key_name, "probe-value")
            .unwrap();
        let fetched = state
            .secrets
            .get_secret("com.talos.desktop", key_name)
            .unwrap();
        assert_eq!(fetched.as_deref(), Some("probe-value"));
        let names = state
            .secrets
            .list_secret_names("com.talos.desktop")
            .unwrap();
        assert!(names.contains(&key_name.to_owned()));
        let statuses = state.llm.lock().await.provider_statuses();
        assert!(statuses.iter().any(|(id, _)| id == "openai"));
        state
            .secrets
            .delete_secret("com.talos.desktop", key_name)
            .unwrap();
        assert_eq!(
            state
                .secrets
                .get_secret("com.talos.desktop", key_name)
                .unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn rejects_unknown_providers_and_bad_keys() {
        let state = test_state().await;
        assert!(list_models_inner(&state, String::new()).await.is_err());
        assert!(delete_provider_key_inner(&state, String::new())
            .await
            .is_err());
        assert!(
            store_provider_key_inner(&state, "nope".to_owned(), "x".to_owned())
                .await
                .is_err()
        );
        assert!(
            store_provider_key_inner(&state, "openai".to_owned(), String::new())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn probe_without_key_reports_unavailable() {
        let state = test_state().await;
        // No key stored and no daemon running: probe fails closed, and the
        // failure message never contains a secret value.
        let result = test_provider_connection_inner(
            &state,
            None,
            "openai".to_owned(),
            Some("gpt-4o-mini".to_owned()),
        )
        .await
        .unwrap();
        assert!(!result.ok);
        assert!(result.latency_ms.is_none());
        assert!(!result.message.is_empty());
    }

    #[tokio::test]
    async fn usage_summary_starts_empty() {
        let state = test_state().await;
        let summary = get_usage_summary_inner(&state).await.unwrap();
        assert_eq!(summary.call_count, 0);
        assert_eq!(summary.total_tokens, 0);
        assert!(summary.by_provider.is_empty());
    }

    #[tokio::test]
    async fn error_codes_map_without_secret_values() {
        let code = llm_error_code(&llm_gateway::LlmError::SecretUnavailable);
        assert_eq!(code, "secret_unavailable");
        let code = llm_error_code(&llm_gateway::LlmError::BudgetExceeded);
        assert_eq!(code, "budget_exceeded");
    }
}
