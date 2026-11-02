//! MCP transports: stdio subprocess, SSE/HTTP, and an in-memory mock.
//!
//! Every transport speaks one JSON-RPC message per request with a deadline.
//! On timeout the stdio child is killed (`Child::kill`) so a hung server can
//! never wedge the caller; SSE requests are dropped and surfaced as timeouts.

use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

use crate::config::McpServerConfig;
use crate::protocol::{JsonRpcRequest, JsonRpcResponse, McpError, RequestId};

/// Something that delivers one JSON-RPC request and returns its response.
#[async_trait::async_trait]
pub trait McpTransport: std::fmt::Debug + Send + Sync {
    /// Send `request` and wait for the matching response before `timeout_ms`.
    async fn round_trip(
        &self,
        request: &JsonRpcRequest,
        timeout_ms: u64,
    ) -> Result<JsonRpcResponse, McpError>;
}

/// Stdio transport: one spawned child per client, line-delimited JSON.
#[derive(Debug)]
pub struct StdioTransport {
    command: String,
    args: Vec<String>,
    env: std::collections::HashMap<String, String>,
}

impl StdioTransport {
    /// Build from a validated stdio config.
    #[must_use]
    pub fn from_config(config: &McpServerConfig) -> Self {
        Self {
            command: config.command.clone().unwrap_or_default(),
            args: config.args.clone(),
            env: config.env.clone(),
        }
    }

    /// Spawn the child with piped stdio.
    fn spawn(&self) -> Result<tokio::process::Child, McpError> {
        let mut child = tokio::process::Command::new(&self.command);
        child.args(&self.args);
        for (key, value) in &self.env {
            child.env(key, value);
        }
        child
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        child
            .spawn()
            .map_err(|err| McpError::transport(format!("spawn {}: {err}", self.command)))
    }
}

#[async_trait::async_trait]
impl McpTransport for StdioTransport {
    async fn round_trip(
        &self,
        request: &JsonRpcRequest,
        timeout_ms: u64,
    ) -> Result<JsonRpcResponse, McpError> {
        request.validate()?;
        let mut child = self.spawn()?;
        let outcome = tokio::time::timeout(
            std::time::Duration::from_millis(timeout_ms),
            exchange_with_child(&mut child, request),
        )
        .await;
        match outcome {
            Ok(result) => result,
            Err(_) => {
                // Deadline: kill the child so a hung server cannot linger.
                let _ = child.kill().await;
                let _ = child.wait().await;
                Err(McpError::timeout(format!(
                    "mcp stdio call to {} timed out after {timeout_ms}ms (server killed)",
                    request.method
                )))
            }
        }
    }
}

/// One write-line / read-line exchange with a live child.
async fn exchange_with_child(
    child: &mut tokio::process::Child,
    request: &JsonRpcRequest,
) -> Result<JsonRpcResponse, McpError> {
    let mut line = serde_json::to_string(request)
        .map_err(|err| McpError::transport(format!("encode request: {err}")))?;
    line.push('\n');
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| McpError::transport("child stdin unavailable".to_owned()))?;
    stdin
        .write_all(line.as_bytes())
        .await
        .map_err(|err| McpError::transport(format!("write child stdin: {err}")))?;
    stdin
        .flush()
        .await
        .map_err(|err| McpError::transport(format!("flush child stdin: {err}")))?;
    drop(stdin);
    let stdout = child
        .stdout
        .as_mut()
        .ok_or_else(|| McpError::transport("child stdout unavailable".to_owned()))?;
    let mut reader = tokio::io::BufReader::new(stdout);
    let mut response_line = String::new();
    reader
        .read_line(&mut response_line)
        .await
        .map_err(|err| McpError::transport(format!("read child stdout: {err}")))?;
    if response_line.trim().is_empty() {
        return Err(McpError::transport(
            "empty response from MCP server".to_owned(),
        ));
    }
    serde_json::from_str(&response_line)
        .map_err(|err| McpError::invalid(format!("decode response: {err}")))
}

/// SSE/HTTP transport: JSON-RPC over HTTP POST to the configured endpoint.
#[derive(Debug, Clone)]
pub struct SseTransport {
    endpoint: String,
}

impl SseTransport {
    /// Build from a validated SSE config.
    #[must_use]
    pub fn from_config(config: &McpServerConfig) -> Self {
        Self {
            endpoint: config.url.clone().unwrap_or_default(),
        }
    }
}

#[async_trait::async_trait]
impl McpTransport for SseTransport {
    async fn round_trip(
        &self,
        request: &JsonRpcRequest,
        timeout_ms: u64,
    ) -> Result<JsonRpcResponse, McpError> {
        request.validate()?;
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(timeout_ms))
            .build()
            .map_err(|err| McpError::transport(format!("http client: {err}")))?;
        let response = client
            .post(&self.endpoint)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .json(request)
            .send()
            .await
            .map_err(|err| {
                if err.is_timeout() {
                    McpError::timeout(format!("mcp SSE call timed out after {timeout_ms}ms"))
                } else {
                    McpError::transport(format!("SSE POST: {err}"))
                }
            })?;
        let status = response.status();
        if !status.is_success() {
            return Err(McpError::transport(format!("SSE HTTP {status}")));
        }
        let raw = response
            .text()
            .await
            .map_err(|err| McpError::transport(format!("SSE body: {err}")))?;
        // Accept either a bare JSON-RPC object or an SSE `data:` frame.
        let payload = raw
            .lines()
            .find_map(|line| {
                let trimmed = line.trim();
                trimmed
                    .strip_prefix("data:")
                    .map(str::trim)
                    .filter(|data| *data != "[DONE]")
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| raw.trim().to_owned());
        // Some servers wrap the response in `{"result": ...}` already; a
        // top-level JSON-RPC envelope is required either way.
        serde_json::from_str(&payload)
            .map_err(|err| McpError::invalid(format!("decode SSE response: {err}")))
    }
}

/// Handler for one mock method: request → response.
pub type MockHandler = dyn Fn(&JsonRpcRequest) -> Result<JsonRpcResponse, McpError> + Send + Sync;

/// In-memory mock transport for tests and policy-gate exercises.
#[derive(Clone)]
pub struct MockTransport {
    /// Handler: method → responder.
    pub handler: std::sync::Arc<MockHandler>,
    /// Artificial delay before responding (tests timeouts deterministically).
    pub delay_ms: u64,
}

impl std::fmt::Debug for MockTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MockTransport")
            .field("delay_ms", &self.delay_ms)
            .finish_non_exhaustive()
    }
}

impl MockTransport {
    /// Build a mock with an optional delay.
    pub fn new(
        handler: impl Fn(&JsonRpcRequest) -> Result<JsonRpcResponse, McpError> + Send + Sync + 'static,
        delay_ms: u64,
    ) -> Self {
        Self {
            handler: std::sync::Arc::new(handler),
            delay_ms,
        }
    }
}

#[async_trait::async_trait]
impl McpTransport for MockTransport {
    async fn round_trip(
        &self,
        request: &JsonRpcRequest,
        timeout_ms: u64,
    ) -> Result<JsonRpcResponse, McpError> {
        request.validate()?;
        if self.delay_ms > timeout_ms {
            tokio::time::sleep(std::time::Duration::from_millis(timeout_ms)).await;
            return Err(McpError::timeout(format!(
                "mock call to {} timed out after {timeout_ms}ms (server killed)",
                request.method
            )));
        }
        if self.delay_ms > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(self.delay_ms)).await;
        }
        (self.handler)(request)
    }
}

/// Response id must mirror the request id (spec conformance check).
pub fn check_id_match(
    request: &JsonRpcRequest,
    response: &JsonRpcResponse,
) -> Result<(), McpError> {
    if request.id != response.id {
        return Err(McpError::invalid(format!(
            "id mismatch: request {:?} vs response {:?}",
            request.id, response.id
        )));
    }
    Ok(())
}

/// Next request id (monotonic per process).
pub fn next_id() -> RequestId {
    RequestId::Str(format!("mcp-{}", uuid::Uuid::new_v4()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::methods;

    fn ok_handler(request: &JsonRpcRequest) -> Result<JsonRpcResponse, McpError> {
        Ok(JsonRpcResponse::ok(
            request.id.clone(),
            serde_json::json!({"echo": request.method}),
        ))
    }

    #[tokio::test]
    async fn mock_round_trip_echoes() {
        let transport = MockTransport::new(ok_handler, 0);
        let request =
            JsonRpcRequest::new(Some(next_id()), methods::TOOLS_LIST, serde_json::json!({}));
        let response = transport.round_trip(&request, 1000).await.unwrap();
        check_id_match(&request, &response).unwrap();
    }

    #[tokio::test]
    async fn mock_timeout_kills_server() {
        let transport = MockTransport::new(ok_handler, 5000);
        let request =
            JsonRpcRequest::new(Some(next_id()), methods::TOOLS_CALL, serde_json::json!({}));
        let err = transport.round_trip(&request, 50).await.unwrap_err();
        assert_eq!(err.code, "timeout");
        assert!(err.message.contains("killed"));
    }

    #[test]
    fn id_mismatch_rejected() {
        let request = JsonRpcRequest::new(
            Some(RequestId::Int(1)),
            methods::TOOLS_LIST,
            serde_json::json!({}),
        );
        let response = JsonRpcResponse::ok(Some(RequestId::Int(2)), serde_json::json!({}));
        assert!(check_id_match(&request, &response).is_err());
    }
}
