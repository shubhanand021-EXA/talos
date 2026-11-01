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

/// Parameters for executing an assembled prompt.
#[derive(Debug)]
pub struct AssemblyExecution<'a> {
    /// Owning task.
    pub task_id: &'a str,
    /// Calling agent.
    pub agent_id: &'a str,
    /// Model id.
    pub model: &'a str,
    /// Model context window in tokens.
    pub context_window: u32,
    /// Assembled prompt.
    pub assembled: &'a context_assembler::AssembledContext,
    /// Tool schemas for the request.
    pub tools: Vec<llm_gateway::ToolSchema>,
    /// Optional spend cap override.
    pub budget_usd: Option<f64>,
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

    /// Execute one task step from an assembled prompt: the assembler runs
    /// first (caller-side), then this method sends the assembled messages.
    ///
    /// When the assembly exceeds the model window the task fails gracefully
    /// with a `context budget exceeded` error and no provider call is made.
    /// Without a gateway this returns [`LlmAgentOutcome::MockFallback`].
    pub async fn execute_task_with_assembly(
        &self,
        execution: AssemblyExecution<'_>,
    ) -> Result<LlmAgentOutcome> {
        let AssemblyExecution {
            task_id,
            agent_id,
            model,
            context_window,
            assembled,
            tools,
            budget_usd,
        } = execution;
        if assembled.total_tokens > context_window {
            return Err(talos_common::TalosError::Storage(format!(
                "context budget exceeded for task {task_id}: {} tokens over {context_window} window",
                assembled.total_tokens
            )));
        }
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
            messages: assembled.messages.clone(),
            tools: if tools.is_empty() { None } else { Some(tools) },
            temperature: Some(0.2),
            max_tokens: Some(1024),
            stop_sequences: Vec::new(),
            metadata: LlmRequestMetadata {
                task_id: task_id.to_owned(),
                agent_id: agent_id.to_owned(),
                purpose: format!("assembled execution {}", assembled.template_version),
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

    fn assembled_for_test(window: u32) -> context_assembler::AssembledContext {
        use context_assembler::{AssemblyInput, ContextAssemblyConfig, GatheredContext};
        let gathered = GatheredContext {
            task_id: "task-9".to_owned(),
            agent_id: "agent-9".to_owned(),
            session_id: "session-9".to_owned(),
            goal: "answer briefly".to_owned(),
            acceptance: Vec::new(),
            constraints: Vec::new(),
            task_title: "t".to_owned(),
            task_status: "executing".to_owned(),
            task_priority: "normal".to_owned(),
            safety_rules: vec!["deny writes without evidence".to_owned()],
            segments: Vec::new(),
            ranked_tools: Vec::new(),
            tool_entries: Vec::new(),
            history: Vec::new(),
            instruction: "say hi".to_owned(),
            chain_head_hash: None,
        };
        context_assembler::assemble(&AssemblyInput {
            gathered,
            model: "gpt-4o-mini".to_owned(),
            context_window: window,
            config: ContextAssemblyConfig::default(),
        })
        .expect("test assembly fits")
    }

    #[tokio::test]
    async fn executes_assembled_prompt_through_gateway() {
        let agent = LlmAgent::new(Some(gateway().await));
        let assembled = assembled_for_test(128_000);
        let outcome = agent
            .execute_task_with_assembly(AssemblyExecution {
                task_id: "task-9",
                agent_id: "agent-9",
                model: "gpt-4o-mini",
                context_window: 128_000,
                assembled: &assembled,
                tools: Vec::new(),
                budget_usd: None,
            })
            .await
            .unwrap();
        match outcome {
            LlmAgentOutcome::Completed { content, .. } => {
                assert_eq!(content.as_deref(), Some("done"));
            }
            LlmAgentOutcome::MockFallback { .. } => panic!("expected completion"),
        }
    }

    #[tokio::test]
    async fn oversized_assembly_fails_gracefully_without_call() {
        let agent = LlmAgent::new(Some(gateway().await));
        // Assembled for a large window, executed against a tiny one.
        let assembled = assembled_for_test(128_000);
        let err = agent
            .execute_task_with_assembly(AssemblyExecution {
                task_id: "task-9",
                agent_id: "agent-9",
                model: "gpt-4o-mini",
                context_window: 4,
                assembled: &assembled,
                tools: Vec::new(),
                budget_usd: None,
            })
            .await
            .unwrap_err();
        assert!(err.to_string().contains("context budget exceeded"));
        assert!(err.to_string().contains("task-9"));
    }
}
