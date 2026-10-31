//! Request/response model for the LLM gateway.
//!
//! Wire shapes mirror `@talos/contracts` `llm.ts` so the TypeScript schemas
//! and these structs can never drift (field names are camelCase on the wire).

use serde::{Deserialize, Serialize};

/// Provider identifier. `Custom` covers self-hosted OpenAI-compatible endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderId {
    /// OpenAI platform (`api.openai.com`).
    OpenAi,
    /// Anthropic API (`api.anthropic.com`).
    Anthropic,
    /// Local Ollama daemon (`localhost:11434`).
    Ollama,
    /// Self-hosted OpenAI-compatible endpoint.
    Custom,
}

impl ProviderId {
    /// Canonical wire id.
    pub fn as_str(&self) -> &'static str {
        match self {
            ProviderId::OpenAi => "openai",
            ProviderId::Anthropic => "anthropic",
            ProviderId::Ollama => "ollama",
            ProviderId::Custom => "custom",
        }
    }

    /// Parse a wire id.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "openai" => Some(ProviderId::OpenAi),
            "anthropic" => Some(ProviderId::Anthropic),
            "ollama" => Some(ProviderId::Ollama),
            "custom" => Some(ProviderId::Custom),
            _ => None,
        }
    }
}

/// Message role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LlmRole {
    /// System instruction.
    System,
    /// End-user input.
    User,
    /// Assistant turn.
    Assistant,
    /// Tool result turn.
    Tool,
}

/// One chat message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmMessage {
    /// Message role.
    pub role: LlmRole,
    /// Text content (never persisted by the gateway; see [`LlmResponse`]).
    pub content: String,
    /// Tool-call id this message answers (tool role only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl LlmMessage {
    /// Build a message with no tool linkage.
    pub fn new(role: LlmRole, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            tool_call_id: None,
        }
    }
}

/// JSON-schema tool description offered to the model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolSchema {
    /// Tool name as the model must invoke it.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// JSON-schema input shape.
    pub input_schema: serde_json::Value,
}

/// Task/agent linkage carried on every request (routing + audit).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmRequestMetadata {
    /// Owning task id (required: no anonymous model calls).
    pub task_id: String,
    /// Calling agent id (required).
    pub agent_id: String,
    /// Free-form purpose label for receipts.
    pub purpose: String,
}

/// A model completion request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmRequest {
    /// Model id (must exist in the registry for the provider).
    pub model: String,
    /// Chat history (oldest first).
    pub messages: Vec<LlmMessage>,
    /// Tools the model may invoke.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolSchema>>,
    /// Sampling temperature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// Cap on generated tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Stop sequences.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stop_sequences: Vec<String>,
    /// Task/agent linkage (required).
    pub metadata: LlmRequestMetadata,
}

/// Prompt/completion token counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    /// Input tokens.
    pub prompt_tokens: u32,
    /// Output tokens.
    pub completion_tokens: u32,
    /// Total tokens.
    pub total_tokens: u32,
}

impl TokenUsage {
    /// Build from parts; total is the sum.
    pub fn new(prompt_tokens: u32, completion_tokens: u32) -> Self {
        Self {
            prompt_tokens,
            completion_tokens,
            total_tokens: prompt_tokens.saturating_add(completion_tokens),
        }
    }
}

/// Why the model stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    /// Natural stop.
    Stop,
    /// Token cap reached.
    Length,
    /// A tool call was emitted.
    ToolCalls,
    /// Content filtered by the provider.
    ContentFilter,
}

/// One model-requested tool invocation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmToolCall {
    /// Stable call id.
    pub id: String,
    /// Tool name.
    pub name: String,
    /// JSON arguments.
    pub arguments: serde_json::Value,
}

/// A model completion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmResponse {
    /// Provider response id.
    pub id: String,
    /// Assistant text (None for pure tool-call turns).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Requested tool invocations.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<LlmToolCall>,
    /// Token counts.
    pub usage: TokenUsage,
    /// Model that actually served the request.
    pub model: String,
    /// Stop reason.
    pub finish_reason: FinishReason,
    /// End-to-end latency in milliseconds.
    pub latency_ms: u64,
}

/// One streamed delta.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmStreamChunk {
    /// Text delta (None for control chunks).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta: Option<String>,
    /// Accumulated usage, present on the final chunk when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<TokenUsage>,
    /// True on the terminal chunk.
    #[serde(default)]
    pub done: bool,
}

/// Handle to a live model stream.
#[derive(Debug)]
pub struct LlmStream {
    /// Ordered deltas (providers collect SSE frames here before returning).
    pub chunks: Vec<LlmStreamChunk>,
}

impl LlmStream {
    /// Wrap pre-collected chunks (mock + collected-SSE providers).
    pub fn collected(chunks: Vec<LlmStreamChunk>) -> Self {
        Self { chunks }
    }

    /// Concatenate all text deltas.
    pub fn text(&self) -> String {
        let mut out = String::new();
        for chunk in &self.chunks {
            if let Some(delta) = &chunk.delta {
                out.push_str(delta);
            }
        }
        out
    }
}

/// Cost/latency receipt for one provider call. Carries no prompt content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderReceipt {
    /// Provider that served the call.
    pub provider_id: String,
    /// Model that served the call.
    pub model_id: String,
    /// Token counts.
    pub usage: TokenUsage,
    /// End-to-end latency in milliseconds.
    pub latency_ms: u64,
    /// Estimated cost in USD.
    pub estimated_cost_usd: f64,
}

impl ProviderReceipt {
    /// Build a receipt, computing cost from per-1k token rates.
    pub fn new(
        provider_id: impl Into<String>,
        model_id: impl Into<String>,
        usage: TokenUsage,
        latency_ms: u64,
        cost_per_1k_input: f64,
        cost_per_1k_output: f64,
    ) -> Self {
        Self {
            provider_id: provider_id.into(),
            model_id: model_id.into(),
            usage,
            latency_ms,
            estimated_cost_usd: estimate_cost(usage, cost_per_1k_input, cost_per_1k_output),
        }
    }
}

/// Linear cost estimate from per-1k token rates.
pub fn estimate_cost(usage: TokenUsage, cost_per_1k_input: f64, cost_per_1k_output: f64) -> f64 {
    f64::max(
        0.0,
        (usage.prompt_tokens as f64 / 1000.0).mul_add(
            cost_per_1k_input,
            usage.completion_tokens as f64 / 1000.0 * cost_per_1k_output,
        ),
    )
}

/// Validate a request before routing. Returns the first defect found.
pub fn validate_request(request: &LlmRequest) -> std::result::Result<(), String> {
    if request.model.is_empty() || request.model.len() > 128 {
        return Err("model must be 1..=128 chars".to_owned());
    }
    if request.messages.is_empty() {
        return Err("messages must not be empty".to_owned());
    }
    for (index, message) in request.messages.iter().enumerate() {
        if message.content.is_empty() {
            return Err(format!("messages[{index}]: content must not be empty"));
        }
    }
    if let Some(temperature) = request.temperature {
        if !(0.0..=2.0).contains(&temperature) {
            return Err("temperature must be within 0.0..=2.0".to_owned());
        }
    }
    if request.metadata.task_id.is_empty() || request.metadata.agent_id.is_empty() {
        return Err("metadata.task_id and metadata.agent_id are required".to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> LlmRequest {
        LlmRequest {
            model: "gpt-4o-mini".to_owned(),
            messages: vec![LlmMessage::new(LlmRole::User, "hello")],
            tools: None,
            temperature: None,
            max_tokens: None,
            stop_sequences: Vec::new(),
            metadata: LlmRequestMetadata {
                task_id: "task-1".to_owned(),
                agent_id: "agent-1".to_owned(),
                purpose: "test".to_owned(),
            },
        }
    }

    #[test]
    fn provider_ids_round_trip() {
        for id in [
            ProviderId::OpenAi,
            ProviderId::Anthropic,
            ProviderId::Ollama,
            ProviderId::Custom,
        ] {
            assert_eq!(ProviderId::parse(id.as_str()), Some(id));
        }
        assert_eq!(ProviderId::parse("nope"), None);
    }

    #[test]
    fn valid_request_passes() {
        assert!(validate_request(&request()).is_ok());
    }

    #[test]
    fn rejects_empty_messages_and_missing_metadata() {
        let mut bad = request();
        bad.messages.clear();
        assert!(validate_request(&bad).is_err());
        let mut bad = request();
        bad.metadata.task_id.clear();
        assert!(validate_request(&bad).is_err());
        let mut bad = request();
        bad.temperature = Some(9.0);
        assert!(validate_request(&bad).is_err());
        let mut bad = request();
        bad.messages[0].content.clear();
        assert!(validate_request(&bad).is_err());
    }

    #[test]
    fn cost_estimate_scales_linearly() {
        let usage = TokenUsage::new(2000, 500);
        let cost = estimate_cost(usage, 1.0, 4.0);
        assert!((cost - 4.0).abs() < f64::EPSILON);
        assert_eq!(estimate_cost(TokenUsage::new(0, 0), 1.0, 1.0), 0.0);
    }

    #[test]
    fn stream_text_concatenates_deltas() {
        let stream = LlmStream::collected(vec![
            LlmStreamChunk {
                delta: Some("he".to_owned()),
                usage: None,
                done: false,
            },
            LlmStreamChunk {
                delta: Some("llo".to_owned()),
                usage: Some(TokenUsage::new(1, 1)),
                done: true,
            },
        ]);
        assert_eq!(stream.text(), "hello");
    }
}
