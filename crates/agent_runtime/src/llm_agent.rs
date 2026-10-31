//! Policy-gated model calls for durable tasks via [`LlmAgent`].
//!
//! The agent holds an optional gateway handle. When a provider key is
//! configured, the agent builds a prompt and routes it through the gateway,
//! which applies policy, budget, and evidence handling before returning the
//! structured result. Without a gateway it returns a mock fallback outcome,
//! so the runtime keeps its deterministic behavior and the caller can emit
//! a warning event.

use std::sync::Arc;

use talos_common::Result;

use llm_gateway::{LlmMessage, LlmRequest, LlmRequestMetadata, LlmRole, LlmStream};

/// System framing for every agent prompt (deterministic, versioned).
pub const LLM_AGENT_SYSTEM_PROMPT: &str = "You are a Talos task agent. Answer concisely and, when a tool is needed, emit a single tool call.";

/// Outcome of one agent execution step.
#[derive(Debug)]
pub enum LlmAgentOutcome {
    /// No provider key configured: deterministic mock path with a warning.
    MockFallback {
        /// Human-readable warning for the event feed.
        warning: String,
        /// Echo-style summary (mirrors `MockEchoAgent::execute_summary`).
        summary: String,
    },
    /// A real provider served the request.
    Completed {
        /// Assistant text, if any.
        content: Option<String>,
        /// Number of tool calls the model requested.
        tool_call_count: usize,
        /// Prompt/completion token counts.
        usage: llm_gateway::TokenUsage,
        /// Estimated cost in USD.
        estimated_cost_usd: f64,
    },
}

/// Model-backed agent. Construct with `None` for deterministic tests.
#[derive(Debug, Default)]
pub struct LlmAgent {
    gateway: Option<Arc<llm_gateway::LlmGateway>>,
}

impl LlmAgent {
    /// Create over an optional gateway handle.
    pub fn new(gateway: Option<Arc<llm_gateway::LlmGateway>>) -> Self {
        Self { gateway }
    }

    /// True when a provider key is configured (gateway present).
    #[must_use]
    pub fn has_provider(&self) -> bool {
        self.gateway.is_some()
    }

    /// Execute one task step: prompt → gateway → structured outcome.
    ///
    /// Without a gateway this returns [`LlmAgentOutcome::MockFallback`]; the
    /// caller emits the warning event and continues deterministically.
    pub async fn execute_task(
        &self,
        task_id: &str,
        agent_id: &str,
        model: &str,
        prompt: &str,
        budget_usd: Option<f64>,
    ) -> Result<LlmAgentOutcome> {
        let Some(gateway) = &self.gateway else {
            return Ok(LlmAgentOutcome::MockFallback {
                warning: format!(
                    "no provider key configured for task {task_id}; using deterministic mock path"
                ),
                summary: format!("echo task {task_id} with no model call"),
            });
        };
        let request = LlmRequest {
            model: model.to_owned(),
            messages: vec![
                LlmMessage::new(LlmRole::System, LLM_AGENT_SYSTEM_PROMPT),
                LlmMessage::new(LlmRole::User, prompt),
            ],
            tools: None,
            temperature: Some(0.2),
            max_tokens: Some(1024),
            stop_sequences: Vec::new(),
            metadata: LlmRequestMetadata {
                task_id: task_id.to_owned(),
                agent_id: agent_id.to_owned(),
                purpose: "agent task execution".to_owned(),
            },
        };
        let (response, receipt) = gateway.complete(request, budget_usd).await?;
        Ok(LlmAgentOutcome::Completed {
            content: response.content,
            tool_call_count: response.tool_calls.len(),
            usage: response.usage,
            estimated_cost_usd: receipt.estimated_cost_usd,
        })
    }

    /// Streaming variant of [`LlmAgent::execute_task`].
    pub async fn execute_task_stream(
        &self,
        task_id: &str,
        agent_id: &str,
        model: &str,
        prompt: &str,
        budget_usd: Option<f64>,
    ) -> Result<(LlmAgentOutcome, Option<LlmStream>)> {
        let Some(gateway) = &self.gateway else {
            return Ok((
                LlmAgentOutcome::MockFallback {
                    warning: format!(
                        "no provider key configured for task {task_id}; using deterministic mock path"
                    ),
                    summary: format!("echo task {task_id} with no model call"),
                },
                None,
            ));
        };
        let request = LlmRequest {
            model: model.to_owned(),
            messages: vec![
                LlmMessage::new(LlmRole::System, LLM_AGENT_SYSTEM_PROMPT),
                LlmMessage::new(LlmRole::User, prompt),
            ],
            tools: None,
            temperature: Some(0.2),
            max_tokens: Some(1024),
            stop_sequences: Vec::new(),
            metadata: LlmRequestMetadata {
                task_id: task_id.to_owned(),
                agent_id: agent_id.to_owned(),
                purpose: "agent task execution".to_owned(),
            },
        };
        let (stream, receipt) = gateway.stream(request, budget_usd).await?;
        Ok((
            LlmAgentOutcome::Completed {
                content: Some(stream.text()),
                tool_call_count: 0,
                usage: llm_gateway::TokenUsage::new(0, 0),
                estimated_cost_usd: receipt.estimated_cost_usd,
            },
            Some(stream),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use llm_gateway::{providers::OpenAiProvider, GatewayConfig, LlmGateway, MockHttpClient};

    fn completion_body() -> serde_json::Value {
        serde_json::json!({
            "id": "chatcmpl-9",
            "model": "gpt-4o-mini",
            "choices": [{
                "message": {"role": "assistant", "content": "done"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 20, "completion_tokens": 4, "total_tokens": 24}
        })
    }

    async fn db() -> evidence_store::EvidenceDb {
        evidence_store::EvidenceDb::connect_memory()
            .await
            .expect("memory db")
    }

    async fn gateway() -> Arc<LlmGateway> {
        let http = Arc::new(MockHttpClient::with_response(200, completion_body()));
        let provider = Arc::new(OpenAiProvider::with_inline_key(
            "https://unit.test/v1",
            "test-key",
            http,
        ));
        Arc::new(LlmGateway::new(
            Arc::new(policy_broker::DefaultPolicyEngine::new()),
            db().await,
            talos_secrets::SecretVault::mock(),
            vec![provider],
            GatewayConfig::default(),
        ))
    }

    #[tokio::test]
    async fn falls_back_without_provider() {
        let agent = LlmAgent::new(None);
        assert!(!agent.has_provider());
        let outcome = agent
            .execute_task("task-1", "agent-1", "gpt-4o-mini", "do it", None)
            .await
            .unwrap();
        match outcome {
            LlmAgentOutcome::MockFallback { warning, .. } => {
                assert!(warning.contains("no provider key configured"));
            }
            LlmAgentOutcome::Completed { .. } => panic!("expected fallback"),
        }
    }

    #[tokio::test]
    async fn completes_through_gateway() {
        let agent = LlmAgent::new(Some(gateway().await));
        assert!(agent.has_provider());
        let outcome = agent
            .execute_task("task-2", "agent-2", "gpt-4o-mini", "do it", None)
            .await
            .unwrap();
        match outcome {
            LlmAgentOutcome::Completed {
                content,
                usage,
                estimated_cost_usd,
                ..
            } => {
                assert_eq!(content.as_deref(), Some("done"));
                assert_eq!(usage.total_tokens, 24);
                assert!(estimated_cost_usd > 0.0);
            }
            LlmAgentOutcome::MockFallback { .. } => panic!("expected completion"),
        }
    }

    #[tokio::test]
    async fn over_budget_denies_without_call() {
        let agent = LlmAgent::new(Some(gateway().await));
        let err = agent
            .execute_task(
                "task-3",
                "agent-3",
                "gpt-4o-mini",
                "do it",
                Some(0.000_000_1),
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("budget"));
    }
}
