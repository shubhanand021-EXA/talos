//! Ollama adapter (`localhost:11434`). No API key; models are discovered
//! dynamically via `/api/tags`.

use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;

use crate::http::HttpClient;
use crate::model::{
    FinishReason, LlmRequest, LlmResponse, LlmStream, LlmStreamChunk, LlmToolCall, TokenUsage,
};
use crate::provider::{LlmError, LlmProvider};

/// Local Ollama daemon base URL.
pub const OLLAMA_DEFAULT_BASE_URL: &str = "http://localhost:11434";

/// Local-first rates: no per-token charge.
pub const OLLAMA_RATES: (f64, f64) = (0.0, 0.0);

/// Ollama chat provider (no keychain involvement).
#[derive(Debug)]
pub struct OllamaProvider {
    base_url: String,
    http: Arc<dyn HttpClient>,
    /// Snapshot from the last `/api/tags` discovery (empty until refreshed).
    known_models: std::sync::Mutex<Vec<String>>,
}

impl OllamaProvider {
    /// Point at a daemon base URL (default `http://localhost:11434`).
    pub fn new(base_url: impl Into<String>, http: Arc<dyn HttpClient>) -> Self {
        Self {
            base_url: base_url.into(),
            http,
            known_models: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Refresh the model catalog from `/api/tags`.
    pub async fn refresh(&self) -> Result<Vec<String>, LlmError> {
        let url = format!("{}/api/tags", self.base_url);
        let (status, body) = self
            .http
            .get(&url, Vec::new())
            .await
            .map_err(LlmError::Transient)?;
        if status != 200 {
            return Err(LlmError::Provider(format!("tags http {status}")));
        }
        let models = parse_tags(&body)?;
        *self
            .known_models
            .lock()
            .map_err(|_| LlmError::Provider("model cache poisoned".into()))? = models.clone();
        Ok(models)
    }

    fn request_body(request: &LlmRequest, stream: bool) -> serde_json::Value {
        serde_json::json!({
            "model": request.model,
            "messages": request.messages.iter().map(|m| serde_json::json!({
                "role": match m.role {
                    crate::model::LlmRole::System => "system",
                    crate::model::LlmRole::User => "user",
                    crate::model::LlmRole::Assistant => "assistant",
                    crate::model::LlmRole::Tool => "tool",
                },
                "content": m.content,
            })).collect::<Vec<_>>(),
            "stream": stream,
            "tools": request.tools.as_ref().map(|tools| tools.iter().map(|t| serde_json::json!({
                "type": "function",
                "function": {
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.input_schema,
                }
            })).collect::<Vec<_>>()),
            "options": {
                "temperature": request.temperature.unwrap_or(0.7),
                "num_predict": request.max_tokens.unwrap_or(1024),
                "stop": request.stop_sequences,
            },
        })
    }

    fn parse_response(model: &str, latency_ms: u64, body: &[u8]) -> Result<LlmResponse, LlmError> {
        let value: serde_json::Value =
            serde_json::from_slice(body).map_err(|_| LlmError::Provider("invalid json".into()))?;
        let message = value
            .get("message")
            .ok_or_else(|| LlmError::Provider("missing message".into()))?;
        let content = message
            .get("content")
            .and_then(|v| v.as_str())
            .map(str::to_owned);
        let mut tool_calls = Vec::new();
        if let Some(calls) = value.get("tool_calls").and_then(|v| v.as_array()) {
            for call in calls {
                let function = call
                    .get("function")
                    .ok_or_else(|| LlmError::Provider("missing tool function".into()))?;
                tool_calls.push(LlmToolCall {
                    id: format!(
                        "ollama-{}",
                        call.get("id").and_then(|v| v.as_str()).unwrap_or("0")
                    ),
                    name: function
                        .get("name")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| LlmError::Provider("missing tool name".into()))?
                        .to_owned(),
                    arguments: function.get("arguments").cloned().unwrap_or_default(),
                });
            }
        }
        let usage = TokenUsage::new(
            value
                .get("prompt_eval_count")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0) as u32,
            value
                .get("eval_count")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0) as u32,
        );
        Ok(LlmResponse {
            id: format!("ollama-{}", unique_suffix()),
            content,
            tool_calls,
            usage,
            model: value
                .get("model")
                .and_then(|v| v.as_str())
                .unwrap_or(model)
                .to_owned(),
            finish_reason: if value
                .get("done_reason")
                .and_then(|v| v.as_str())
                .is_some_and(|r| r == "length")
            {
                FinishReason::Length
            } else if tool_calls_empty(&value) {
                FinishReason::Stop
            } else {
                FinishReason::ToolCalls
            },
            latency_ms,
        })
    }
}

fn tool_calls_empty(value: &serde_json::Value) -> bool {
    value
        .get("tool_calls")
        .and_then(|v| v.as_array())
        .is_none_or(|a| a.is_empty())
}

fn unique_suffix() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0)
        .to_string()
}

/// Parse `/api/tags` into model names.
pub fn parse_tags(body: &[u8]) -> Result<Vec<String>, LlmError> {
    let value: serde_json::Value =
        serde_json::from_slice(body).map_err(|_| LlmError::Provider("invalid tags json".into()))?;
    let models = value
        .get("models")
        .and_then(|v| v.as_array())
        .ok_or_else(|| LlmError::Provider("missing models".into()))?;
    let mut names = Vec::new();
    for model in models {
        if let Some(name) = model.get("name").and_then(|v| v.as_str()) {
            names.push(name.to_owned());
        }
    }
    Ok(names)
}

/// Parse Ollama NDJSON chat frames into text chunks.
pub fn parse_ollama_ndjson(body: &[u8]) -> Result<Vec<LlmStreamChunk>, LlmError> {
    let text = std::str::from_utf8(body).map_err(|_| LlmError::Provider("invalid utf8".into()))?;
    let mut chunks = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(line)
            .map_err(|_| LlmError::Provider("bad ndjson frame".into()))?;
        let delta = value
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .map(str::to_owned);
        let done = value.get("done").and_then(|v| v.as_bool()).unwrap_or(false);
        chunks.push(LlmStreamChunk {
            delta,
            usage: None,
            done,
        });
        if done {
            break;
        }
    }
    Ok(chunks)
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    fn provider_id(&self) -> &str {
        "ollama"
    }

    fn supports_model(&self, model: &str) -> bool {
        self.known_models
            .lock()
            .map(|known| known.iter().any(|m| m == model))
            .unwrap_or(false)
            || model.starts_with("llama")
            || model.starts_with("mistral")
            || model.starts_with("qwen")
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_tool_calls(&self) -> bool {
        true
    }

    fn supports_vision(&self) -> bool {
        false
    }

    async fn refresh_models(&self) -> Result<Vec<String>, LlmError> {
        self.refresh().await
    }

    fn discovered_models(&self) -> Vec<String> {
        self.known_models
            .lock()
            .map(|known| known.clone())
            .unwrap_or_default()
    }

    async fn complete(&self, request: LlmRequest) -> Result<LlmResponse, LlmError> {
        let started = Instant::now();
        let url = format!("{}/api/chat", self.base_url);
        let (status, body) = self
            .http
            .post_json(&url, Vec::new(), Self::request_body(&request, false))
            .await
            .map_err(LlmError::Transient)?;
        if status != 200 {
            return Err(LlmError::Provider(format!("chat http {status}")));
        }
        Self::parse_response(&request.model, started.elapsed().as_millis() as u64, &body)
    }

    async fn stream(&self, request: LlmRequest) -> Result<LlmStream, LlmError> {
        let url = format!("{}/api/chat", self.base_url);
        let (status, body) = self
            .http
            .post_json(&url, Vec::new(), Self::request_body(&request, true))
            .await
            .map_err(LlmError::Transient)?;
        if status != 200 {
            return Err(LlmError::Provider(format!("chat http {status}")));
        }
        Ok(LlmStream::collected(parse_ollama_ndjson(&body)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::MockHttpClient;
    use crate::model::{LlmMessage, LlmRequestMetadata, LlmRole};

    fn request() -> LlmRequest {
        LlmRequest {
            model: "llama3".to_owned(),
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

    #[tokio::test]
    async fn discovers_models_from_tags() {
        let http = Arc::new(MockHttpClient::with_response(
            200,
            serde_json::json!({"models": [{"name": "llama3:latest"}, {"name": "mistral:7b"}]}),
        ));
        let provider = OllamaProvider::new("http://unit.test", http);
        let models = provider.refresh_models().await.unwrap();
        assert_eq!(models, vec!["llama3:latest", "mistral:7b"]);
        assert!(provider.supports_model("llama3:latest"));
    }

    #[tokio::test]
    async fn completes_with_eval_counts() {
        let http = Arc::new(MockHttpClient::with_response(
            200,
            serde_json::json!({
                "model": "llama3",
                "message": {"role": "assistant", "content": "hey"},
                "done": true,
                "prompt_eval_count": 12,
                "eval_count": 6
            }),
        ));
        let provider = OllamaProvider::new("http://unit.test", http);
        let response = provider.complete(request()).await.unwrap();
        assert_eq!(response.content.as_deref(), Some("hey"));
        assert_eq!(response.usage, TokenUsage::new(12, 6));
    }

    #[test]
    fn parses_ndjson_stream() {
        let body = b"{\"message\":{\"content\":\"he\"},\"done\":false}\n{\"message\":{\"content\":\"llo\"},\"done\":true}\n";
        let chunks = parse_ollama_ndjson(body).unwrap();
        assert_eq!(
            chunks
                .iter()
                .filter_map(|c| c.delta.clone())
                .collect::<String>(),
            "hello"
        );
        assert!(chunks.last().is_some_and(|c| c.done));
    }

    #[test]
    fn rejects_malformed_tags_and_chat() {
        assert!(parse_tags(b"{\"nope\":true}").is_err());
        assert!(parse_ollama_ndjson(b"not json\n").is_err());
    }
}
