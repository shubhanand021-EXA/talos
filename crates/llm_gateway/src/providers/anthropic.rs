//! Anthropic adapter (`api.anthropic.com`).
//!
//! Key handling mirrors the OpenAI adapter: keychain per call, header-only
//! transport, never logged or persisted.

use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use talos_secrets::{provider_key_name, SecretVault, TALOS_SERVICE};

use crate::http::HttpClient;
use crate::model::{
    FinishReason, LlmMessage, LlmRequest, LlmResponse, LlmStream, LlmStreamChunk, LlmToolCall,
    TokenUsage,
};
use crate::provider::{LlmError, LlmProvider};

/// Models served through the Anthropic messages API.
pub const ANTHROPIC_MODELS: &[&str] = &["claude-sonnet-4-20250514", "claude-3-5-haiku"];

/// API version pinned for the messages endpoint.
pub const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Per-1k-token USD list prices keyed by model (input, output).
pub fn anthropic_rates(model: &str) -> (f64, f64) {
    match model {
        "claude-sonnet-4-20250514" => (0.003, 0.015),
        "claude-3-5-haiku" => (0.0008, 0.004),
        _ => (0.003, 0.015),
    }
}

/// Anthropic messages provider.
#[derive(Debug)]
pub struct AnthropicProvider {
    base_url: String,
    vault: SecretVault,
    inline_key: Option<String>,
    http: Arc<dyn HttpClient>,
}

impl AnthropicProvider {
    /// Production constructor: key comes from the OS keychain per call.
    pub fn new(vault: SecretVault, http: Arc<dyn HttpClient>) -> Self {
        Self {
            base_url: "https://api.anthropic.com/v1".to_owned(),
            vault,
            inline_key: None,
            http,
        }
    }

    /// Test constructor with an inline (fake) key. Never used in production.
    pub fn with_inline_key(key: impl Into<String>, http: Arc<dyn HttpClient>) -> Self {
        Self {
            base_url: "https://unit.test/v1".to_owned(),
            vault: SecretVault::mock(),
            inline_key: Some(key.into()),
            http,
        }
    }

    fn api_key(&self) -> Result<String, LlmError> {
        if let Some(key) = &self.inline_key {
            return Ok(key.clone());
        }
        self.vault
            .get_secret(TALOS_SERVICE, &provider_key_name("anthropic"))
            .map_err(|_| LlmError::SecretUnavailable)?
            .ok_or(LlmError::SecretUnavailable)
    }

    fn request_body(request: &LlmRequest, stream: bool) -> serde_json::Value {
        let system: Vec<&LlmMessage> = request
            .messages
            .iter()
            .filter(|m| matches!(m.role, crate::model::LlmRole::System))
            .collect();
        let messages: Vec<serde_json::Value> = request
            .messages
            .iter()
            .filter(|m| !matches!(m.role, crate::model::LlmRole::System))
            .map(|m| {
                serde_json::json!({
                    "role": match m.role {
                        crate::model::LlmRole::Assistant => "assistant",
                        crate::model::LlmRole::Tool => "user",
                        _ => "user",
                    },
                    "content": m.content,
                })
            })
            .collect();
        let mut body = serde_json::json!({
            "model": request.model,
            "messages": messages,
            "max_tokens": request.max_tokens.unwrap_or(1024),
            "stream": stream,
        });
        if !system.is_empty() {
            body["system"] = serde_json::json!(system
                .iter()
                .map(|m| m.content.clone())
                .collect::<Vec<_>>()
                .join("\n"));
        }
        if let Some(tools) = &request.tools {
            body["tools"] = serde_json::json!(tools
                .iter()
                .map(|t| serde_json::json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.input_schema,
                }))
                .collect::<Vec<_>>());
        }
        if let Some(temperature) = request.temperature {
            body["temperature"] = serde_json::json!(temperature);
        }
        if !request.stop_sequences.is_empty() {
            body["stop_sequences"] = serde_json::json!(request.stop_sequences);
        }
        body
    }

    fn map_status(status: u16, detail: &str) -> LlmError {
        match status {
            401 => LlmError::AuthFailure,
            429 => LlmError::RateLimited,
            500..=599 => LlmError::Transient(format!("http {status}: {detail}")),
            _ => LlmError::Provider(format!("http {status}: {detail}")),
        }
    }

    fn parse_response(model: &str, latency_ms: u64, body: &[u8]) -> Result<LlmResponse, LlmError> {
        let value: serde_json::Value =
            serde_json::from_slice(body).map_err(|_| LlmError::Provider("invalid json".into()))?;
        let id = value
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| LlmError::Provider("missing id".into()))?;
        let blocks = value
            .get("content")
            .and_then(|v| v.as_array())
            .ok_or_else(|| LlmError::Provider("missing content".into()))?;
        let mut text = String::new();
        let mut tool_calls = Vec::new();
        for block in blocks {
            match block.get("type").and_then(|v| v.as_str()) {
                Some("text") => {
                    if let Some(part) = block.get("text").and_then(|v| v.as_str()) {
                        text.push_str(part);
                    }
                }
                Some("tool_use") => {
                    let call_id = block
                        .get("id")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| LlmError::Provider("missing tool id".into()))?;
                    let name = block
                        .get("name")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| LlmError::Provider("missing tool name".into()))?;
                    tool_calls.push(LlmToolCall {
                        id: call_id.to_owned(),
                        name: name.to_owned(),
                        arguments: block.get("input").cloned().unwrap_or_default(),
                    });
                }
                _ => {}
            }
        }
        let usage_value = value.get("usage").cloned().unwrap_or_default();
        let usage = TokenUsage::new(
            usage_value
                .get("input_tokens")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0) as u32,
            usage_value
                .get("output_tokens")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0) as u32,
        );
        let stop_reason = value
            .get("stop_reason")
            .and_then(|v| v.as_str())
            .unwrap_or("end_turn");
        let finish_reason = match stop_reason {
            "max_tokens" => FinishReason::Length,
            "tool_use" => FinishReason::ToolCalls,
            _ => FinishReason::Stop,
        };
        Ok(LlmResponse {
            id: id.to_owned(),
            content: if text.is_empty() { None } else { Some(text) },
            tool_calls,
            usage,
            model: value
                .get("model")
                .and_then(|v| v.as_str())
                .unwrap_or(model)
                .to_owned(),
            finish_reason,
            latency_ms,
        })
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    fn provider_id(&self) -> &str {
        "anthropic"
    }

    fn supports_model(&self, model: &str) -> bool {
        ANTHROPIC_MODELS.contains(&model)
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_tool_calls(&self) -> bool {
        true
    }

    fn supports_vision(&self) -> bool {
        true
    }

    async fn complete(&self, request: LlmRequest) -> Result<LlmResponse, LlmError> {
        let started = Instant::now();
        let headers = vec![
            ("x-api-key".to_owned(), self.api_key()?),
            ("anthropic-version".to_owned(), ANTHROPIC_VERSION.to_owned()),
            ("Content-Type".to_owned(), "application/json".to_owned()),
        ];
        let url = format!("{}/messages", self.base_url);
        let (status, body) = self
            .http
            .post_json(&url, headers, Self::request_body(&request, false))
            .await
            .map_err(|err| {
                if err == "timeout" {
                    LlmError::Timeout
                } else {
                    LlmError::Transient(err)
                }
            })?;
        if status != 200 {
            return Err(Self::map_status(status, "anthropic messages"));
        }
        Self::parse_response(&request.model, started.elapsed().as_millis() as u64, &body)
    }

    async fn stream(&self, request: LlmRequest) -> Result<LlmStream, LlmError> {
        let headers = vec![
            ("x-api-key".to_owned(), self.api_key()?),
            ("anthropic-version".to_owned(), ANTHROPIC_VERSION.to_owned()),
            ("Content-Type".to_owned(), "application/json".to_owned()),
            ("Accept".to_owned(), "text/event-stream".to_owned()),
        ];
        let url = format!("{}/messages", self.base_url);
        let (status, body) = self
            .http
            .post_json(&url, headers, Self::request_body(&request, true))
            .await
            .map_err(|err| {
                if err == "timeout" {
                    LlmError::Timeout
                } else {
                    LlmError::Transient(err)
                }
            })?;
        if status != 200 {
            return Err(Self::map_status(status, "anthropic stream"));
        }
        Ok(LlmStream::collected(parse_anthropic_sse(&body)?))
    }
}

/// Parse Anthropic SSE: `content_block_delta` data lines carry `delta.text`;
/// `message_stop` terminates.
pub fn parse_anthropic_sse(body: &[u8]) -> Result<Vec<LlmStreamChunk>, LlmError> {
    let text = std::str::from_utf8(body).map_err(|_| LlmError::Provider("invalid utf8".into()))?;
    let mut chunks = Vec::new();
    for frame in text.split("\n\n") {
        let mut event = "";
        let mut data: Option<&str> = None;
        for line in frame.lines() {
            if let Some(name) = line.strip_prefix("event:") {
                event = name.trim();
            } else if let Some(payload) = line.strip_prefix("data:") {
                data = Some(payload.trim());
            }
        }
        let Some(data) = data else { continue };
        if event == "message_stop" {
            chunks.push(LlmStreamChunk {
                delta: None,
                usage: None,
                done: true,
            });
            return Ok(chunks);
        }
        let value: serde_json::Value =
            serde_json::from_str(data).map_err(|_| LlmError::Provider("bad sse frame".into()))?;
        if let Some(part) = value
            .get("delta")
            .and_then(|d| d.get("text"))
            .and_then(|t| t.as_str())
        {
            chunks.push(LlmStreamChunk {
                delta: Some(part.to_owned()),
                usage: None,
                done: false,
            });
        }
    }
    Ok(chunks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::MockHttpClient;
    use crate::model::{LlmMessage, LlmRequestMetadata, LlmRole};

    fn request() -> LlmRequest {
        LlmRequest {
            model: "claude-3-5-haiku".to_owned(),
            messages: vec![
                LlmMessage::new(LlmRole::System, "be brief"),
                LlmMessage::new(LlmRole::User, "hi"),
            ],
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

    fn message_body() -> serde_json::Value {
        serde_json::json!({
            "id": "msg-1",
            "type": "message",
            "model": "claude-3-5-haiku",
            "content": [{"type": "text", "text": "hello"}],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 8, "output_tokens": 4}
        })
    }

    #[tokio::test]
    async fn completes_text_and_usage() {
        let http = Arc::new(MockHttpClient::with_response(200, message_body()));
        let provider = AnthropicProvider::with_inline_key("test-key", http);
        let response = provider.complete(request()).await.unwrap();
        assert_eq!(response.content.as_deref(), Some("hello"));
        assert_eq!(response.usage, TokenUsage::new(8, 4));
    }

    #[tokio::test]
    async fn parses_tool_use_blocks() {
        let mut body = message_body();
        body["content"] = serde_json::json!([{
            "type": "tool_use", "id": "tu-1",
            "name": "fs.list", "input": {"dir": "/tmp"}
        }]);
        body["stop_reason"] = serde_json::json!("tool_use");
        let http = Arc::new(MockHttpClient::with_response(200, body));
        let provider = AnthropicProvider::with_inline_key("test-key", http);
        let response = provider.complete(request()).await.unwrap();
        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(response.tool_calls[0].name, "fs.list");
        assert_eq!(response.finish_reason, FinishReason::ToolCalls);
    }

    #[tokio::test]
    async fn missing_keychain_secret_is_unavailable() {
        let http = Arc::new(MockHttpClient::with_response(200, message_body()));
        let provider = AnthropicProvider::new(SecretVault::mock(), http);
        let err = provider.complete(request()).await.unwrap_err();
        assert!(matches!(err, LlmError::SecretUnavailable));
    }

    #[test]
    fn parses_sse_deltas_until_stop() {
        let body = b"event: content_block_delta\ndata: {\"delta\":{\"text\":\"he\"}}\n\nevent: content_block_delta\ndata: {\"delta\":{\"text\":\"llo\"}}\n\nevent: message_stop\ndata: {}\n\n";
        let chunks = parse_anthropic_sse(body).unwrap();
        assert_eq!(
            chunks
                .iter()
                .filter_map(|c| c.delta.clone())
                .collect::<String>(),
            "hello"
        );
        assert!(chunks.iter().any(|c| c.done));
    }

    #[test]
    fn capability_flags() {
        let provider = AnthropicProvider::with_inline_key("k", Arc::new(MockHttpClient::default()));
        assert_eq!(provider.provider_id(), "anthropic");
        assert!(provider.supports_model("claude-sonnet-4-20250514"));
        assert!(!provider.supports_model("gpt-4o"));
    }
}
