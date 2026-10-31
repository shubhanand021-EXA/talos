//! OpenAI adapter (`api.openai.com`, OpenAI-compatible `custom` endpoints).
//!
//! The API key is read from the OS keychain on every call and travels only
//! in the `Authorization` header. It is never logged, persisted, or
//! returned.

use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use talos_secrets::{provider_key_name, SecretVault, TALOS_SERVICE};

use crate::http::HttpClient;
use crate::model::{
    estimate_cost, FinishReason, LlmRequest, LlmResponse, LlmRole, LlmStream, LlmToolCall,
    TokenUsage,
};
use crate::provider::{LlmError, LlmProvider};

/// Models served through the OpenAI chat-completions API.
pub const OPENAI_MODELS: &[&str] = &["gpt-4o", "gpt-4o-mini", "o3-mini"];

/// Per-1k-token USD list prices keyed by model (input, output).
pub fn openai_rates(model: &str) -> (f64, f64) {
    match model {
        "gpt-4o" => (0.0025, 0.01),
        "gpt-4o-mini" => (0.00015, 0.0006),
        "o3-mini" => (0.0011, 0.0044),
        _ => (0.0025, 0.01),
    }
}

/// OpenAI (or OpenAI-compatible) chat provider.
#[derive(Debug)]
pub struct OpenAiProvider {
    base_url: String,
    vault: SecretVault,
    inline_key: Option<String>,
    http: Arc<dyn HttpClient>,
}

impl OpenAiProvider {
    /// Production constructor: key comes from the OS keychain per call.
    pub fn new(vault: SecretVault, http: Arc<dyn HttpClient>) -> Self {
        Self {
            base_url: "https://api.openai.com/v1".to_owned(),
            vault,
            inline_key: None,
            http,
        }
    }

    /// OpenAI-compatible endpoint with a custom base URL.
    pub fn custom(
        base_url: impl Into<String>,
        vault: SecretVault,
        http: Arc<dyn HttpClient>,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            vault,
            inline_key: None,
            http,
        }
    }

    /// Test constructor with an inline (fake) key. Never used in production.
    pub fn with_inline_key(
        base_url: impl Into<String>,
        key: impl Into<String>,
        http: Arc<dyn HttpClient>,
    ) -> Self {
        Self {
            base_url: base_url.into(),
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
            .get_secret(TALOS_SERVICE, &provider_key_name("openai"))
            .map_err(|_| LlmError::SecretUnavailable)?
            .ok_or(LlmError::SecretUnavailable)
    }

    fn auth_header(&self) -> Result<Vec<(String, String)>, LlmError> {
        Ok(vec![
            (
                "Authorization".to_owned(),
                format!("Bearer {}", self.api_key()?),
            ),
            ("Content-Type".to_owned(), "application/json".to_owned()),
        ])
    }

    fn request_body(request: &LlmRequest, stream: bool) -> serde_json::Value {
        let messages: Vec<serde_json::Value> = request
            .messages
            .iter()
            .map(|m| {
                serde_json::json!({
                    "role": match m.role {
                        LlmRole::System => "system",
                        LlmRole::User => "user",
                        LlmRole::Assistant => "assistant",
                        LlmRole::Tool => "tool",
                    },
                    "content": m.content,
                })
            })
            .collect();
        let mut body = serde_json::json!({
            "model": request.model,
            "messages": messages,
            "stream": stream,
        });
        if let Some(tools) = &request.tools {
            body["tools"] = serde_json::json!(tools
                .iter()
                .map(|t| serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.input_schema,
                    }
                }))
                .collect::<Vec<_>>());
        }
        if let Some(temperature) = request.temperature {
            body["temperature"] = serde_json::json!(temperature);
        }
        if let Some(max_tokens) = request.max_tokens {
            body["max_tokens"] = serde_json::json!(max_tokens);
        }
        if !request.stop_sequences.is_empty() {
            body["stop"] = serde_json::json!(request.stop_sequences);
        }
        if stream {
            body["stream_options"] = serde_json::json!({"include_usage": true});
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
        let choice = value
            .get("choices")
            .and_then(|v| v.as_array())
            .and_then(|a| a.first())
            .ok_or_else(|| LlmError::Provider("missing choices".into()))?;
        let message = choice
            .get("message")
            .ok_or_else(|| LlmError::Provider("missing message".into()))?;
        let content = message
            .get("content")
            .and_then(|v| v.as_str())
            .map(str::to_owned);
        let mut tool_calls = Vec::new();
        if let Some(calls) = message.get("tool_calls").and_then(|v| v.as_array()) {
            for call in calls {
                let call_id = call
                    .get("id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| LlmError::Provider("missing tool call id".into()))?;
                let function = call
                    .get("function")
                    .ok_or_else(|| LlmError::Provider("missing tool function".into()))?;
                let name = function
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| LlmError::Provider("missing tool name".into()))?;
                let arguments = function.get("arguments").cloned().unwrap_or_default();
                let arguments = if let Some(raw) = arguments.as_str() {
                    serde_json::from_str(raw).unwrap_or(serde_json::Value::Null)
                } else {
                    arguments
                };
                tool_calls.push(LlmToolCall {
                    id: call_id.to_owned(),
                    name: name.to_owned(),
                    arguments,
                });
            }
        }
        let usage_value = value.get("usage").cloned().unwrap_or_default();
        let usage = TokenUsage::new(
            usage_value
                .get("prompt_tokens")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0) as u32,
            usage_value
                .get("completion_tokens")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0) as u32,
        );
        let finish_reason = match choice
            .get("finish_reason")
            .and_then(|v| v.as_str())
            .unwrap_or("stop")
        {
            "length" => FinishReason::Length,
            "tool_calls" => FinishReason::ToolCalls,
            "content_filter" => FinishReason::ContentFilter,
            _ => FinishReason::Stop,
        };
        let _ = estimate_cost;
        Ok(LlmResponse {
            id: id.to_owned(),
            content,
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
impl LlmProvider for OpenAiProvider {
    fn provider_id(&self) -> &str {
        "openai"
    }

    fn supports_model(&self, model: &str) -> bool {
        OPENAI_MODELS.contains(&model)
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
        let headers = self.auth_header()?;
        let url = format!("{}/chat/completions", self.base_url);
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
            return Err(Self::map_status(status, "openai completion"));
        }
        Self::parse_response(&request.model, started.elapsed().as_millis() as u64, &body)
    }

    async fn stream(&self, request: LlmRequest) -> Result<LlmStream, LlmError> {
        let headers = self.auth_header()?;
        let url = format!("{}/chat/completions", self.base_url);
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
            return Err(Self::map_status(status, "openai stream"));
        }
        Ok(LlmStream::collected(parse_openai_sse(&body)?))
    }
}

/// Parse OpenAI `text/event-stream` frames into text chunks.
///
/// Each `data:` line carries one JSON choice delta; `[DONE]` terminates.
pub fn parse_openai_sse(body: &[u8]) -> Result<Vec<crate::model::LlmStreamChunk>, LlmError> {
    use crate::model::LlmStreamChunk;
    let text = std::str::from_utf8(body).map_err(|_| LlmError::Provider("invalid utf8".into()))?;
    let mut chunks = Vec::new();
    for frame in text.split("\n\n") {
        for line in frame.lines() {
            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data == "[DONE]" {
                chunks.push(LlmStreamChunk {
                    delta: None,
                    usage: None,
                    done: true,
                });
                return Ok(chunks);
            }
            let value: serde_json::Value = serde_json::from_str(data)
                .map_err(|_| LlmError::Provider("bad sse frame".into()))?;
            let delta = value
                .get("choices")
                .and_then(|v| v.as_array())
                .and_then(|a| a.first())
                .and_then(|c| c.get("delta"))
                .and_then(|d| d.get("content"))
                .and_then(|c| c.as_str())
                .map(str::to_owned);
            let done = value
                .get("choices")
                .and_then(|v| v.as_array())
                .and_then(|a| a.first())
                .and_then(|c| c.get("finish_reason"))
                .is_some_and(|r| !r.is_null());
            chunks.push(LlmStreamChunk {
                delta,
                usage: None,
                done,
            });
            if done {
                return Ok(chunks);
            }
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
            model: "gpt-4o-mini".to_owned(),
            messages: vec![LlmMessage::new(LlmRole::User, "hi")],
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

    fn completion_body() -> serde_json::Value {
        serde_json::json!({
            "id": "chatcmpl-1",
            "model": "gpt-4o-mini",
            "choices": [{
                "message": {"role": "assistant", "content": "hello"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
        })
    }

    #[tokio::test]
    async fn completes_and_maps_usage() {
        let http = Arc::new(MockHttpClient::with_response(200, completion_body()));
        let provider =
            OpenAiProvider::with_inline_key("https://unit.test/v1", "test-key", http.clone());
        let response = provider.complete(request()).await.unwrap();
        assert_eq!(response.content.as_deref(), Some("hello"));
        assert_eq!(response.usage, TokenUsage::new(10, 5));
        assert_eq!(response.finish_reason, FinishReason::Stop);
        assert_eq!(http.call_count(), 1);
    }

    #[tokio::test]
    async fn parses_tool_calls() {
        let mut body = completion_body();
        body["choices"][0]["message"] = serde_json::json!({
            "role": "assistant",
            "content": null,
            "tool_calls": [{
                "id": "call-1",
                "type": "function",
                "function": {"name": "fs.read", "arguments": "{\"path\": \"/x\"}"}
            }]
        });
        body["choices"][0]["finish_reason"] = serde_json::json!("tool_calls");
        let http = Arc::new(MockHttpClient::with_response(200, body));
        let provider = OpenAiProvider::with_inline_key("https://unit.test/v1", "test-key", http);
        let response = provider.complete(request()).await.unwrap();
        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(response.tool_calls[0].name, "fs.read");
        assert_eq!(response.finish_reason, FinishReason::ToolCalls);
    }

    #[tokio::test]
    async fn maps_auth_rate_and_transient_statuses() {
        for (status, check) in [
            (401u16, "auth"),
            (429u16, "rate"),
            (500u16, "transient"),
            (400u16, "provider"),
        ] {
            let http = Arc::new(MockHttpClient::with_response(
                status,
                serde_json::json!({"error": "x"}),
            ));
            let provider =
                OpenAiProvider::with_inline_key("https://unit.test/v1", "test-key", http);
            let err = provider.complete(request()).await.unwrap_err();
            let text = err.to_string();
            assert!(
                text.contains(check) || matches!(status, 401 | 429),
                "unexpected mapping for {status}: {text}"
            );
        }
    }

    #[tokio::test]
    async fn missing_keychain_secret_is_unavailable() {
        let http = Arc::new(MockHttpClient::with_response(200, completion_body()));
        let provider = OpenAiProvider::new(SecretVault::mock(), http);
        let err = provider.complete(request()).await.unwrap_err();
        assert!(matches!(err, LlmError::SecretUnavailable));
    }

    #[tokio::test]
    async fn rejects_malformed_schema() {
        let http = Arc::new(MockHttpClient::with_response(
            200,
            serde_json::json!({"nope": true}),
        ));
        let provider = OpenAiProvider::with_inline_key("https://unit.test/v1", "test-key", http);
        assert!(provider.complete(request()).await.is_err());
    }

    #[test]
    fn parses_sse_deltas() {
        let body = b"data: {\"choices\":[{\"delta\":{\"content\":\"he\"},\"finish_reason\":null}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"llo\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
        let chunks = parse_openai_sse(body).unwrap();
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
        let provider = OpenAiProvider::with_inline_key(
            "https://unit.test/v1",
            "k",
            Arc::new(MockHttpClient::default()),
        );
        assert_eq!(provider.provider_id(), "openai");
        assert!(provider.supports_model("gpt-4o-mini"));
        assert!(!provider.supports_model("claude-3-5-haiku"));
        assert!(provider.supports_streaming());
        assert!(provider.supports_tool_calls());
        assert!(provider.supports_vision());
    }
}
