//! [`LlmGateway`]: validate → budget → evidence → policy → execute → record.
//!
//! Every call records request/response *metadata* (hashes, counts, cost) as
//! evidence and usage. Raw prompts and completions never leave the provider
//! call — the ledger stays private by construction.

use std::collections::HashMap;
use std::sync::Arc;

use policy_broker::{
    ActionRequest, Capability, PolicyContext, PolicyDecision, PolicyEngine, RiskLevel,
};
use talos_common::{now_iso, AgentId, TaskId};

use crate::budget::BudgetTracker;
use crate::model::{
    estimate_cost, validate_request, LlmRequest, LlmResponse, LlmStream, ProviderReceipt,
    TokenUsage,
};
use crate::provider::{LlmError, LlmProvider};
use crate::registry::{default_registry, ModelDescriptor, ModelRouter};

/// Retry/backoff tuning for transient provider failures.
#[derive(Debug, Clone)]
pub struct GatewayConfig {
    /// Total attempts including the first (1 = no retry).
    pub max_attempts: u32,
    /// Base backoff in milliseconds (doubles per attempt).
    pub base_backoff_ms: u64,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_backoff_ms: 500,
        }
    }
}

/// Policy-gated, budget-enforced entry point to every model call.
#[derive(Debug)]
pub struct LlmGateway {
    engine: Arc<dyn PolicyEngine>,
    db: evidence_store::EvidenceDb,
    vault: talos_secrets::SecretVault,
    providers: HashMap<String, Arc<dyn LlmProvider>>,
    router: ModelRouter,
    config: GatewayConfig,
}

impl LlmGateway {
    /// Build with an explicit provider set (tests inject mocks here).
    pub fn new(
        engine: Arc<dyn PolicyEngine>,
        db: evidence_store::EvidenceDb,
        vault: talos_secrets::SecretVault,
        providers: Vec<Arc<dyn LlmProvider>>,
        config: GatewayConfig,
    ) -> Self {
        let mut table = HashMap::new();
        for provider in providers {
            table.insert(provider.provider_id().to_owned(), provider);
        }
        Self {
            engine,
            db,
            vault,
            providers: table,
            router: default_registry(),
            config,
        }
    }

    /// IDs of registered providers, sorted.
    pub fn provider_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.providers.keys().cloned().collect();
        ids.sort();
        ids
    }

    /// Descriptor catalog, cheapest first.
    pub fn catalog(&self) -> Vec<ModelDescriptor> {
        self.router.catalog().into_iter().cloned().collect()
    }

    /// Descriptors for one provider, cheapest first. Best-effort refreshes
    /// Ollama's daemon catalog first; discovered models merge with zero
    /// rates so the UI catalog reflects the local daemon.
    pub async fn models_for(&self, provider_id: &str) -> Vec<ModelDescriptor> {
        let Some(provider) = self.providers.get(provider_id) else {
            return Vec::new();
        };
        let _ = provider.refresh_models().await;
        let mut merged: Vec<ModelDescriptor> = self
            .catalog()
            .into_iter()
            .filter(|d| d.provider_id == provider_id)
            .collect();
        for discovered in provider.discovered_models() {
            let short = discovered
                .split(':')
                .next()
                .unwrap_or(&discovered)
                .to_owned();
            if !merged
                .iter()
                .any(|d| d.model_id == discovered || d.model_id == short)
            {
                merged.push(ModelDescriptor {
                    provider_id: provider_id.to_owned(),
                    model_id: discovered,
                    context_window: 8192,
                    supports_tools: true,
                    supports_vision: false,
                    supports_streaming: true,
                    cost_per_1k_input: 0.0,
                    cost_per_1k_output: 0.0,
                });
            }
        }
        merged.sort_by(|a, b| {
            a.blended_rate()
                .partial_cmp(&b.blended_rate())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        merged
    }

    /// `(provider_id, key_configured)` for every registered provider.
    /// Key *values* never leave the vault; this reports presence only.
    pub fn provider_statuses(&self) -> Vec<(String, bool)> {
        let mut statuses: Vec<(String, bool)> = self
            .providers
            .keys()
            .map(|id| {
                let configured = if id == "ollama" {
                    true
                } else {
                    self.vault
                        .get_secret(
                            talos_secrets::TALOS_SERVICE,
                            &talos_secrets::provider_key_name(id),
                        )
                        .unwrap_or(None)
                        .is_some()
                };
                (id.clone(), configured)
            })
            .collect();
        statuses.sort_by(|a, b| a.0.cmp(&b.0));
        statuses
    }

    /// Minimal live probe: one short completion. Returns
    /// `(served, latency_ms, message)`. Secret values never surface.
    pub async fn test_connection(
        &self,
        provider_id: &str,
        model: Option<&str>,
    ) -> (bool, Option<u64>, String) {
        let started = std::time::Instant::now();
        let chosen = match model {
            Some(explicit) => explicit.to_owned(),
            None => match self.models_for(provider_id).await.first() {
                Some(descriptor) => descriptor.model_id.clone(),
                None => {
                    return (
                        false,
                        None,
                        format!("no models known for provider {provider_id}"),
                    );
                }
            },
        };
        let request = LlmRequest {
            model: chosen,
            messages: vec![crate::model::LlmMessage::new(
                crate::model::LlmRole::User,
                "Say hello in five words or fewer.",
            )],
            tools: None,
            temperature: Some(0.0),
            max_tokens: Some(32),
            stop_sequences: Vec::new(),
            metadata: crate::model::LlmRequestMetadata {
                task_id: "connection-probe".to_owned(),
                agent_id: "connection-probe".to_owned(),
                purpose: "provider connectivity probe".to_owned(),
            },
        };
        match self.complete(request, None).await {
            Ok((_, receipt)) => (
                true,
                Some(started.elapsed().as_millis() as u64),
                format!(
                    "served by {} in {} ms ({} tokens)",
                    receipt.model_id, receipt.latency_ms, receipt.usage.total_tokens
                ),
            ),
            Err(err) => (false, None, err.to_string()),
        }
    }

    /// Resolve the adapter serving `model` (`provider/model` prefix supported).
    pub fn provider_for(&self, model: &str) -> Result<Arc<dyn LlmProvider>, LlmError> {
        if let Some((prefix, _)) = model.split_once('/') {
            return self
                .providers
                .get(prefix)
                .cloned()
                .ok_or_else(|| LlmError::ModelUnavailable(format!("unknown provider {prefix}")));
        }
        let mut matches = self
            .providers
            .values()
            .filter(|p| p.supports_model(model))
            .cloned();
        match (matches.next(), matches.next()) {
            (Some(only), None) => Ok(only),
            (Some(_), Some(_)) => Err(LlmError::ModelUnavailable(format!(
                "ambiguous model {model}: qualify as provider/model"
            ))),
            _ => Err(LlmError::ModelUnavailable(format!(
                "no provider serves {model}"
            ))),
        }
    }

    /// One-shot completion with full gating. Returns response + receipt.
    ///
    /// Telemetry wrapper: records a `talos.llm_gateway.model_call` span
    /// (provider, model, tokens, latency, cost — never prompt content).
    pub async fn complete(
        &self,
        request: LlmRequest,
        budget_override_usd: Option<f64>,
    ) -> Result<(LlmResponse, ProviderReceipt), LlmError> {
        let started = std::time::Instant::now();
        let task_id = request.metadata.task_id.clone();
        let agent_id = request.metadata.agent_id.clone();
        let model = request.model.clone();
        let outcome = self.complete_inner(request, budget_override_usd).await;
        let duration_ms = started.elapsed().as_millis().clamp(0, i64::MAX as u128) as i64;
        match &outcome {
            Ok((_, receipt)) => talos_telemetry::record_now(
                "llm_gateway",
                "model_call",
                Some(&task_id),
                Some(&agent_id),
                talos_telemetry::SpanStatus::Ok,
                "served",
                duration_ms,
                &[
                    ("model.provider", serde_json::json!(receipt.provider_id)),
                    ("model.id", serde_json::json!(receipt.model_id)),
                    (
                        "tokens.prompt",
                        serde_json::json!(receipt.usage.prompt_tokens),
                    ),
                    (
                        "tokens.completion",
                        serde_json::json!(receipt.usage.completion_tokens),
                    ),
                    (
                        "tokens.total",
                        serde_json::json!(receipt.usage.total_tokens),
                    ),
                    ("cost.usd", serde_json::json!(receipt.estimated_cost_usd)),
                    ("latency_ms", serde_json::json!(receipt.latency_ms)),
                ],
            ),
            Err(err) => talos_telemetry::record_now(
                "llm_gateway",
                "model_call",
                Some(&task_id),
                Some(&agent_id),
                talos_telemetry::SpanStatus::Error,
                "call failed",
                duration_ms,
                &[
                    ("model.id", serde_json::json!(model)),
                    (
                        "error.code",
                        serde_json::json!(err.to_string().chars().take(128).collect::<String>()),
                    ),
                ],
            ),
        }
        outcome
    }

    /// Inner completion: gate → execute → record.
    async fn complete_inner(
        &self,
        request: LlmRequest,
        budget_override_usd: Option<f64>,
    ) -> Result<(LlmResponse, ProviderReceipt), LlmError> {
        let planned = self.gate(&request, budget_override_usd).await?;
        let response = self
            .execute_with_retry(&planned.provider, request.clone())
            .await?;
        let receipt = self.record(&request, &planned, &response).await?;
        Ok((response, receipt))
    }

    /// Streaming completion with full gating.
    pub async fn stream(
        &self,
        request: LlmRequest,
        budget_override_usd: Option<f64>,
    ) -> Result<(LlmStream, ProviderReceipt), LlmError> {
        let planned = self.gate(&request, budget_override_usd).await?;
        let stream = self
            .execute_stream_with_retry(&planned.provider, request.clone())
            .await?;
        // Streams report usage when the provider includes it; otherwise fall
        // back to a documented character-based estimate.
        let usage = stream
            .chunks
            .iter()
            .filter_map(|c| c.usage)
            .next_back()
            .unwrap_or_else(|| {
                TokenUsage::new(
                    estimate_tokens(&request_text(&request)),
                    estimate_tokens(&stream.text()),
                )
            });
        let response = LlmResponse {
            id: format!("stream-{}", planned.request_id),
            content: Some(stream.text()),
            tool_calls: Vec::new(),
            usage,
            model: request.model.clone(),
            finish_reason: crate::model::FinishReason::Stop,
            latency_ms: 0,
        };
        let receipt = self.record(&request, &planned, &response).await?;
        Ok((stream, receipt))
    }

    /// Shared pre-execution gate: validate → budget → evidence → policy.
    async fn gate(
        &self,
        request: &LlmRequest,
        budget_override_usd: Option<f64>,
    ) -> Result<PlannedCall, LlmError> {
        validate_request(request).map_err(LlmError::InvalidRequest)?;
        // Valid task/agent linkage: no anonymous model calls.
        TaskId::new(request.metadata.task_id.clone())
            .map_err(|err| LlmError::InvalidRequest(err.to_string()))?;
        AgentId::new(request.metadata.agent_id.clone())
            .map_err(|err| LlmError::InvalidRequest(err.to_string()))?;
        let provider = self.provider_for(&request.model)?;
        let descriptor = self
            .router
            .lookup(provider.provider_id(), &short_model(&request.model))
            .cloned();

        // Budget: explicit override wins; otherwise the task's ledger cap
        // (`None` = uncapped).
        let tracker = BudgetTracker::new(&self.db);
        let cap = match budget_override_usd {
            Some(cap) => Some(cap),
            None => evidence_store::get_task_budget(&self.db, &request.metadata.task_id)
                .await
                .unwrap_or(None),
        };
        let projected = estimate_cost(
            TokenUsage::new(
                estimate_tokens(&request_text(request)),
                request.max_tokens.unwrap_or(1024),
            ),
            descriptor
                .as_ref()
                .map(|d| d.cost_per_1k_input)
                .unwrap_or(0.01),
            descriptor
                .as_ref()
                .map(|d| d.cost_per_1k_output)
                .unwrap_or(0.03),
        );
        if tracker
            .would_exceed(&request.metadata.task_id, cap, projected)
            .await
            .map_err(LlmError::Storage)?
        {
            return Err(LlmError::BudgetExceeded);
        }

        // Record the request receipt first so the policy gate sees evidence.
        let request_id = format!("llm-{}-{}", request.metadata.task_id, short_id());
        let action = ActionRequest {
            id: request_id.clone(),
            agent_id: request.metadata.agent_id.clone(),
            task_id: request.metadata.task_id.clone(),
            capability: Capability::NetworkRequest,
            intent: format!(
                "llm:{}:{}",
                provider.provider_id(),
                request.metadata.purpose
            ),
            target: format!("llm://{}/{}", provider.provider_id(), request.model),
            payload_json: serde_json::json!({
                "provider": provider.provider_id(),
                "model": request.model,
                "purpose": request.metadata.purpose,
                "message_count": request.messages.len(),
                "tool_count": request.tools.as_ref().map(|t| t.len()).unwrap_or(0),
            }),
            risk_level: RiskLevel::Medium,
            reversible: true,
            requested_at: chrono::Utc::now(),
        };
        let evidence_id = format!("llm-evidence-{request_id}");
        evidence_store::insert_evidence(
            &self.db,
            evidence_store::NewEvidence {
                id: evidence_id.clone(),
                task_id: Some(request.metadata.task_id.clone()),
                agent_id: Some(request.metadata.agent_id.clone()),
                session_id: Some("session-local".to_owned()),
                kind: "llm.request".to_owned(),
                label: Some(format!("{} {}", provider.provider_id(), request.model)),
                summary: Some(format!(
                    "model request: {} messages via {}",
                    request.messages.len(),
                    provider.provider_id()
                )),
                uri: None,
                mime_type: None,
                payload: serde_json::to_value(&action.payload_json)
                    .map_err(|err| LlmError::Storage(err.to_string()))?,
            },
        )
        .await
        .map_err(|err| LlmError::Storage(err.to_string()))?;
        evidence_store::transition_evidence_status(&self.db, &evidence_id, "verified")
            .await
            .map_err(|err| LlmError::Storage(err.to_string()))?;

        let evaluation = self.engine.evaluate_with_context(
            &action,
            &PolicyContext {
                has_evidence: true,
                has_compensation_plan: false,
            },
        );
        evidence_store::record_action_request(
            &self.db,
            &evidence_store::NewActionRecord {
                id: action.id.clone(),
                agent_id: action.agent_id.clone(),
                task_id: action.task_id.clone(),
                capability: format!("{:?}", action.capability),
                intent: action.intent.clone(),
                target: action.target.clone(),
                payload: action.payload_json.clone(),
                risk_level: format!("{:?}", action.risk_level),
                reversible: action.reversible,
                requested_at: action.requested_at.to_rfc3339(),
            },
            "talos-policy-broker/1",
            &format!("{:?}", evaluation.decision),
            &evaluation.reason,
            &evaluation.required_checks,
        )
        .await
        .map_err(|err| LlmError::Storage(err.to_string()))?;
        if evaluation.decision != PolicyDecision::Allow {
            return Err(LlmError::PolicyDenied(evaluation.reason));
        }

        Ok(PlannedCall {
            request_id,
            provider,
            descriptor,
        })
    }

    async fn execute_with_retry(
        &self,
        provider: &Arc<dyn LlmProvider>,
        request: LlmRequest,
    ) -> Result<LlmResponse, LlmError> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            match provider.complete(request.clone()).await {
                Ok(response) => return Ok(response),
                Err(err) if err.is_retryable() && attempt < self.config.max_attempts => {
                    let backoff = self
                        .config
                        .base_backoff_ms
                        .saturating_mul(1 << (attempt - 1));
                    tokio::time::sleep(std::time::Duration::from_millis(backoff)).await;
                }
                Err(err) => return Err(err),
            }
        }
    }

    async fn execute_stream_with_retry(
        &self,
        provider: &Arc<dyn LlmProvider>,
        request: LlmRequest,
    ) -> Result<LlmStream, LlmError> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            match provider.stream(request.clone()).await {
                Ok(stream) => return Ok(stream),
                Err(err) if err.is_retryable() && attempt < self.config.max_attempts => {
                    let backoff = self
                        .config
                        .base_backoff_ms
                        .saturating_mul(1 << (attempt - 1));
                    tokio::time::sleep(std::time::Duration::from_millis(backoff)).await;
                }
                Err(err) => return Err(err),
            }
        }
    }

    /// Persist usage + interaction hashes; build the cost receipt.
    async fn record(
        &self,
        request: &LlmRequest,
        planned: &PlannedCall,
        response: &LlmResponse,
    ) -> Result<ProviderReceipt, LlmError> {
        let (input_rate, output_rate) = planned
            .descriptor
            .as_ref()
            .map(|d| (d.cost_per_1k_input, d.cost_per_1k_output))
            .unwrap_or((0.01, 0.03));
        let receipt = ProviderReceipt::new(
            planned.provider.provider_id(),
            response.model.clone(),
            response.usage,
            response.latency_ms,
            input_rate,
            output_rate,
        );
        evidence_store::insert_usage_record(
            &self.db,
            &evidence_store::NewUsageRecord {
                id: &format!("usage-{}", planned.request_id),
                task_id: &request.metadata.task_id,
                agent_id: &request.metadata.agent_id,
                provider_id: planned.provider.provider_id(),
                model_id: &response.model,
                prompt_tokens: response.usage.prompt_tokens,
                completion_tokens: response.usage.completion_tokens,
                total_tokens: response.usage.total_tokens,
                estimated_cost_usd: receipt.estimated_cost_usd,
                latency_ms: response.latency_ms,
            },
        )
        .await
        .map_err(|err| LlmError::Storage(err.to_string()))?;
        // Hashes of canonical metadata only — never raw content.
        let request_meta = serde_json::json!({
            "provider": planned.provider.provider_id(),
            "model": request.model,
            "purpose": request.metadata.purpose,
            "message_count": request.messages.len(),
            "tool_count": request.tools.as_ref().map(|t| t.len()).unwrap_or(0),
        });
        let response_meta = serde_json::json!({
            "model": response.model,
            "finish_reason": format!("{:?}", response.finish_reason),
            "prompt_tokens": response.usage.prompt_tokens,
            "completion_tokens": response.usage.completion_tokens,
            "tool_call_count": response.tool_calls.len(),
        });
        evidence_store::insert_model_interaction(
            &self.db,
            &evidence_store::NewModelInteraction {
                id: &format!("interaction-{}", planned.request_id),
                task_id: &request.metadata.task_id,
                agent_id: &request.metadata.agent_id,
                provider_id: planned.provider.provider_id(),
                model_id: &response.model,
                request_hash: &evidence_store::hash_request_metadata(&request_meta),
                response_hash: &evidence_store::hash_request_metadata(&response_meta),
                finish_reason: &format!("{:?}", response.finish_reason),
            },
        )
        .await
        .map_err(|err| LlmError::Storage(err.to_string()))?;
        Ok(receipt)
    }

    /// Vault handle for keychain-backed administration commands.
    pub fn vault(&self) -> &talos_secrets::SecretVault {
        &self.vault
    }
}

/// A call that cleared validation, budget, and policy.
struct PlannedCall {
    request_id: String,
    provider: Arc<dyn LlmProvider>,
    descriptor: Option<ModelDescriptor>,
}

/// Strip an optional `provider/` prefix for catalog lookup.
fn short_model(model: &str) -> String {
    model
        .split_once('/')
        .map(|(_, rest)| rest.to_owned())
        .unwrap_or_else(|| model.to_owned())
}

/// Documented character-based token estimate (fallback only).
fn estimate_tokens(text: &str) -> u32 {
    (text.chars().count() as u32 / 4).max(1)
}

fn request_text(request: &LlmRequest) -> String {
    request
        .messages
        .iter()
        .map(|m| m.content.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn short_id() -> String {
    now_iso().replace([':', '.', '-'], "")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::MockHttpClient;
    use crate::providers::OpenAiProvider;

    fn engine() -> Arc<dyn PolicyEngine> {
        Arc::new(policy_broker::DefaultPolicyEngine::new())
    }

    async fn db() -> evidence_store::EvidenceDb {
        evidence_store::EvidenceDb::connect_memory()
            .await
            .expect("memory db")
    }

    fn request(model: &str) -> LlmRequest {
        LlmRequest {
            model: model.to_owned(),
            messages: vec![crate::model::LlmMessage::new(
                crate::model::LlmRole::User,
                "hello",
            )],
            tools: None,
            temperature: None,
            max_tokens: None,
            stop_sequences: Vec::new(),
            metadata: crate::model::LlmRequestMetadata {
                task_id: "task-1".to_owned(),
                agent_id: "agent-1".to_owned(),
                purpose: "unit test".to_owned(),
            },
        }
    }

    fn completion_body() -> serde_json::Value {
        serde_json::json!({
            "id": "chatcmpl-1",
            "model": "gpt-4o-mini",
            "choices": [{
                "message": {"role": "assistant", "content": "hi"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
        })
    }

    async fn gateway_with(body: serde_json::Value, status: u16) -> LlmGateway {
        let http = Arc::new(MockHttpClient::with_response(status, body));
        let provider: Arc<dyn LlmProvider> = Arc::new(OpenAiProvider::with_inline_key(
            "https://unit.test/v1",
            "test-key",
            http,
        ));
        LlmGateway::new(
            engine(),
            db().await,
            talos_secrets::SecretVault::mock(),
            vec![provider],
            GatewayConfig {
                max_attempts: 3,
                base_backoff_ms: 1,
            },
        )
    }

    #[tokio::test]
    async fn rejects_invalid_requests_before_policy() {
        let gateway = gateway_with(completion_body(), 200).await;
        let mut bad = request("gpt-4o-mini");
        bad.messages.clear();
        assert!(matches!(
            gateway.complete(bad, None).await.unwrap_err(),
            LlmError::InvalidRequest(_)
        ));
        let mut bad = request("gpt-4o-mini");
        bad.metadata.task_id.clear();
        assert!(matches!(
            gateway.complete(bad, None).await.unwrap_err(),
            LlmError::InvalidRequest(_)
        ));
    }

    #[tokio::test]
    async fn rejects_unknown_models() {
        let gateway = gateway_with(completion_body(), 200).await;
        let err = gateway.complete(request("nope-9"), None).await.unwrap_err();
        assert!(matches!(err, LlmError::ModelUnavailable(_)));
    }

    #[tokio::test]
    async fn denies_over_budget_before_any_call() {
        let http = Arc::new(MockHttpClient::with_response(200, completion_body()));
        let provider: Arc<dyn LlmProvider> = Arc::new(OpenAiProvider::with_inline_key(
            "https://unit.test/v1",
            "test-key",
            http.clone(),
        ));
        let db_handle = db().await;
        evidence_store::insert_usage_record(
            &db_handle,
            &evidence_store::NewUsageRecord {
                id: "u-spent",
                task_id: "task-1",
                agent_id: "agent-1",
                provider_id: "openai",
                model_id: "gpt-4o-mini",
                prompt_tokens: 1000,
                completion_tokens: 0,
                total_tokens: 1000,
                estimated_cost_usd: 0.01,
                latency_ms: 10,
            },
        )
        .await
        .unwrap();
        let gateway = LlmGateway::new(
            engine(),
            db_handle,
            talos_secrets::SecretVault::mock(),
            vec![provider],
            GatewayConfig::default(),
        );
        let err = gateway
            .complete(request("gpt-4o-mini"), Some(0.005))
            .await
            .unwrap_err();
        assert!(matches!(err, LlmError::BudgetExceeded));
        assert_eq!(http.call_count(), 0);
    }

    #[tokio::test]
    async fn records_usage_and_interaction_hashes() {
        let gateway = gateway_with(completion_body(), 200).await;
        let (response, receipt) = gateway
            .complete(request("gpt-4o-mini"), None)
            .await
            .unwrap();
        assert_eq!(response.usage, TokenUsage::new(10, 5));
        assert!(receipt.estimated_cost_usd > 0.0);
        let (tokens, cost, calls) = evidence_store::summarize_usage(&gateway.db).await.unwrap();
        assert_eq!(tokens, 15);
        assert!(cost > 0.0);
        assert_eq!(calls, 1);
        let interactions = sqlx::query_as::<_, evidence_store::ModelInteractionRow>(
            "SELECT * FROM model_interactions",
        )
        .fetch_all(gateway.db.pool())
        .await
        .unwrap();
        assert_eq!(interactions.len(), 1);
        assert!(interactions[0].request_hash.starts_with("sha256:"));
        // Policy audit trail recorded the gated call.
        let evaluations = sqlx::query_as::<_, evidence_store::PolicyEvaluationRow>(
            "SELECT * FROM policy_evaluations",
        )
        .fetch_all(gateway.db.pool())
        .await
        .unwrap();
        assert!(evaluations.iter().any(|e| e.decision == "Allow"));
    }

    #[tokio::test]
    async fn retries_transient_then_succeeds() {
        // Queue is FIFO: seed the failure first, then the success.
        let http = Arc::new(MockHttpClient::default());
        http.push_response(500, serde_json::json!({"error": "boom"}));
        http.push_response(200, completion_body());
        let provider: Arc<dyn LlmProvider> = Arc::new(OpenAiProvider::with_inline_key(
            "https://unit.test/v1",
            "test-key",
            http.clone(),
        ));
        let gateway = LlmGateway::new(
            engine(),
            db().await,
            talos_secrets::SecretVault::mock(),
            vec![provider],
            GatewayConfig {
                max_attempts: 3,
                base_backoff_ms: 1,
            },
        );
        let (response, _) = gateway
            .complete(request("gpt-4o-mini"), None)
            .await
            .unwrap();
        assert_eq!(response.content.as_deref(), Some("hi"));
        assert_eq!(http.call_count(), 2);
    }

    #[tokio::test]
    async fn propagates_auth_failure_without_retry() {
        let http = Arc::new(MockHttpClient::with_response(
            401,
            serde_json::json!({"error": "bad key"}),
        ));
        let provider: Arc<dyn LlmProvider> = Arc::new(OpenAiProvider::with_inline_key(
            "https://unit.test/v1",
            "wrong-key",
            http.clone(),
        ));
        let gateway = LlmGateway::new(
            engine(),
            db().await,
            talos_secrets::SecretVault::mock(),
            vec![provider],
            GatewayConfig::default(),
        );
        let err = gateway
            .complete(request("gpt-4o-mini"), None)
            .await
            .unwrap_err();
        assert!(matches!(err, LlmError::AuthFailure));
        assert_eq!(http.call_count(), 1);
    }
}
