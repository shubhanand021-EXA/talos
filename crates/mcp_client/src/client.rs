//! MCP client: handshake, discovery, and policy-gated tool invocation.
//!
//! Every `tools/call` is built as a [`policy_broker::ActionRequest`] first
//! (capability `NetworkRequest`, medium risk): the caller evaluates it with
//! the policy broker before any byte reaches the server, and the receipt is
//! persisted as evidence by the caller. The client itself never bypasses
//! policy — it only transports already-authorized calls.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::config::{McpServerConfig, TransportKind};
use crate::protocol::{
    methods, CallToolResult, InitializeResult, JsonRpcRequest, JsonRpcResponse, McpError,
    McpPrompt, McpResource, McpTool, RequestId,
};
use crate::transport::{check_id_match, McpTransport, MockTransport, SseTransport, StdioTransport};

/// Connection state for one MCP server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServerStatus {
    /// Never contacted.
    Disconnected,
    /// Handshake completed.
    Connected,
    /// Last operation failed.
    Error,
}

/// MCP client bound to one configured server.
pub struct McpClient {
    /// Server name (config key).
    pub name: String,
    /// Validated server config.
    pub config: McpServerConfig,
    transport: Box<dyn McpTransport>,
    next_numeric_id: AtomicU64,
    status: std::sync::Mutex<ServerStatus>,
    last_error: std::sync::Mutex<Option<String>>,
}

impl std::fmt::Debug for McpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpClient")
            .field("name", &self.name)
            .field("config", &self.config)
            .field("status", &self.status.lock().ok().map(|guard| *guard))
            .finish()
    }
}

impl McpClient {
    /// Build a client from a validated config entry.
    pub fn from_config(name: &str, config: McpServerConfig) -> Result<Self, McpError> {
        config.validate(name)?;
        let transport: Box<dyn McpTransport> = match config.transport {
            TransportKind::Stdio => Box::new(StdioTransport::from_config(&config)),
            TransportKind::Sse => Box::new(SseTransport::from_config(&config)),
        };
        Ok(Self {
            name: name.to_owned(),
            config,
            transport,
            next_numeric_id: AtomicU64::new(1),
            status: std::sync::Mutex::new(ServerStatus::Disconnected),
            last_error: std::sync::Mutex::new(None),
        })
    }

    /// Build a client over a mock transport (tests, policy exercises).
    pub fn from_mock(
        name: &str,
        config: McpServerConfig,
        mock: MockTransport,
    ) -> Result<Self, McpError> {
        config.validate(name)?;
        Ok(Self {
            name: name.to_owned(),
            config,
            transport: Box::new(mock),
            next_numeric_id: AtomicU64::new(1),
            status: std::sync::Mutex::new(ServerStatus::Disconnected),
            last_error: std::sync::Mutex::new(None),
        })
    }

    /// Current connection status.
    #[must_use]
    pub fn status(&self) -> ServerStatus {
        self.status
            .lock()
            .ok()
            .map(|guard| *guard)
            .unwrap_or(ServerStatus::Error)
    }

    /// Last error message, if any.
    #[must_use]
    pub fn last_error(&self) -> Option<String> {
        self.last_error.lock().ok().and_then(|guard| guard.clone())
    }

    fn note_success(&self) {
        if let Ok(mut status) = self.status.lock() {
            *status = ServerStatus::Connected;
        }
        if let Ok(mut last) = self.last_error.lock() {
            *last = None;
        }
    }

    fn note_failure(&self, message: &str) {
        if let Ok(mut status) = self.status.lock() {
            *status = ServerStatus::Error;
        }
        if let Ok(mut last) = self.last_error.lock() {
            *last = Some(message.chars().take(512).collect());
        }
    }

    fn next_id(&self) -> RequestId {
        RequestId::Int(self.next_numeric_id.fetch_add(1, Ordering::Relaxed) as i64)
    }

    /// Low-level request with timeout from the server config.
    async fn send(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, McpError> {
        let started = std::time::Instant::now();
        let request = JsonRpcRequest::new(Some(self.next_id()), method, params);
        let outcome = self
            .transport
            .round_trip(&request, self.config.effective_timeout_ms())
            .await;
        let duration_ms = started.elapsed().as_millis().clamp(0, i64::MAX as u128) as i64;
        let task_label = format!("mcp:{}", self.name);
        match outcome {
            Ok(response) => {
                if let Err(err) = check_id_match(&request, &response) {
                    self.note_failure(&err.message);
                    record_span(&task_label, method, duration_ms, false);
                    return Err(err);
                }
                if let Some(error) = response.error {
                    self.note_failure(&error.message);
                    record_span(&task_label, method, duration_ms, false);
                    return Err(McpError::new(
                        &format!("remote-{}", error.code),
                        error.message,
                    ));
                }
                self.note_success();
                record_span(&task_label, method, duration_ms, true);
                Ok(response.result.unwrap_or(serde_json::Value::Null))
            }
            Err(err) => {
                self.note_failure(&err.message);
                record_span(&task_label, method, duration_ms, false);
                Err(err)
            }
        }
    }

    /// `initialize` handshake: negotiate the protocol version and record
    /// server identity. Must be called before discovery.
    pub async fn initialize(&self) -> Result<InitializeResult, McpError> {
        let result = self
            .send(
                methods::INITIALIZE,
                serde_json::json!({
                    "protocolVersion": methods::PROTOCOL_VERSION,
                    "clientName": "talos",
                    "clientVersion": env!("CARGO_PKG_VERSION"),
                    "capabilities": self.config.capabilities,
                }),
            )
            .await?;
        serde_json::from_value(result)
            .map_err(|err| McpError::invalid(format!("decode initialize: {err}")))
    }

    /// `tools/list`: discover tool schemas from the server.
    pub async fn list_tools(&self) -> Result<Vec<McpTool>, McpError> {
        let result = self
            .send(methods::TOOLS_LIST, serde_json::json!({}))
            .await?;
        let tools: Vec<McpTool> = serde_json::from_value(
            result
                .get("tools")
                .cloned()
                .unwrap_or(serde_json::json!([])),
        )
        .map_err(|err| McpError::invalid(format!("decode tools/list: {err}")))?;
        for tool in &tools {
            tool.validate()?;
        }
        Ok(tools)
    }

    /// Build the policy-broker request for `tools/call` (evaluate before send).
    #[must_use]
    pub fn tool_action_request(
        &self,
        task_id: &str,
        agent_id: &str,
        tool: &str,
        arguments: &serde_json::Value,
    ) -> policy_broker::ActionRequest {
        policy_broker::ActionRequest {
            id: format!("mcp-{}-{tool}-{}", self.name, uuid::Uuid::new_v4()),
            agent_id: agent_id.to_owned(),
            task_id: task_id.to_owned(),
            capability: policy_broker::Capability::NetworkRequest,
            intent: format!(
                "mcp {} tool {tool} via server {}",
                methods::TOOLS_CALL,
                self.name
            ),
            target: format!("mcp:{}:{tool}", self.name),
            payload_json: serde_json::json!({"tool": tool, "arguments": arguments}),
            risk_level: policy_broker::RiskLevel::Medium,
            reversible: false,
            requested_at: chrono::Utc::now(),
        }
    }

    /// `tools/call`: invoke a tool. The caller must have an `Allow` verdict
    /// for [`McpClient::tool_action_request`] — passing `authorized = false`
    /// denies locally without touching the transport.
    pub async fn call_tool(
        &self,
        tool: &str,
        arguments: serde_json::Value,
        authorized: bool,
    ) -> Result<CallToolResult, McpError> {
        if tool.trim().is_empty() || tool.len() > 128 {
            return Err(McpError::invalid("tool name must be 1..=128 chars"));
        }
        if !authorized {
            return Err(McpError::denied(format!(
                "mcp tool {tool} on {} denied: no policy Allow verdict",
                self.name
            )));
        }
        let result = self
            .send(
                methods::TOOLS_CALL,
                serde_json::json!({"name": tool, "arguments": arguments}),
            )
            .await?;
        serde_json::from_value(result)
            .map_err(|err| McpError::invalid(format!("decode tools/call: {err}")))
    }

    /// `resources/list`: discover resources.
    pub async fn list_resources(&self) -> Result<Vec<McpResource>, McpError> {
        let result = self
            .send(methods::RESOURCES_LIST, serde_json::json!({}))
            .await?;
        serde_json::from_value(
            result
                .get("resources")
                .cloned()
                .unwrap_or(serde_json::json!([])),
        )
        .map_err(|err| McpError::invalid(format!("decode resources/list: {err}")))
    }

    /// `resources/read`: read one resource by URI.
    pub async fn read_resource(&self, uri: &str) -> Result<serde_json::Value, McpError> {
        if uri.trim().is_empty() || uri.len() > 2048 {
            return Err(McpError::invalid("resource uri must be 1..=2048 chars"));
        }
        self.send(methods::RESOURCES_READ, serde_json::json!({"uri": uri}))
            .await
    }

    /// `prompts/list`: discover prompts.
    pub async fn list_prompts(&self) -> Result<Vec<McpPrompt>, McpError> {
        let result = self
            .send(methods::PROMPTS_LIST, serde_json::json!({}))
            .await?;
        serde_json::from_value(
            result
                .get("prompts")
                .cloned()
                .unwrap_or(serde_json::json!([])),
        )
        .map_err(|err| McpError::invalid(format!("decode prompts/list: {err}")))
    }

    /// `prompts/get`: fetch one prompt by name.
    pub async fn get_prompt(
        &self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value, McpError> {
        if name.trim().is_empty() || name.len() > 128 {
            return Err(McpError::invalid("prompt name must be 1..=128 chars"));
        }
        self.send(
            methods::PROMPTS_GET,
            serde_json::json!({"name": name, "arguments": arguments}),
        )
        .await
    }
}

#[allow(dead_code)]
fn response_for_test_harness(_response: &JsonRpcResponse) {}

/// Telemetry: one span per MCP round trip (metadata only, never payloads).
fn record_span(task_label: &str, method: &str, duration_ms: i64, ok: bool) {
    talos_telemetry::record_now(
        "mcp_client",
        "round_trip",
        Some(task_label),
        None,
        if ok {
            talos_telemetry::SpanStatus::Ok
        } else {
            talos_telemetry::SpanStatus::Error
        },
        method,
        duration_ms,
        &[("mcp.method", serde_json::json!(method))],
    );
}

/// Evidence payload for an MCP tool call receipt (metadata + hashes only).
#[must_use]
pub fn tool_call_evidence(
    server: &str,
    tool: &str,
    task_id: &str,
    agent_id: &str,
    decision: &policy_broker::PolicyDecision,
    reason: &str,
    output_hash: &str,
) -> serde_json::Value {
    serde_json::json!({
        "server": server,
        "tool": tool,
        "task_id": task_id,
        "agent_id": agent_id,
        "decision": format!("{decision:?}"),
        "reason": reason,
        "output_hash": output_hash,
        "recorded_at": talos_common::now_iso(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::JsonRpcResponse;
    use std::collections::HashMap;

    fn config() -> McpServerConfig {
        McpServerConfig {
            command: Some("mock".to_owned()),
            args: Vec::new(),
            env: HashMap::new(),
            url: None,
            transport: TransportKind::Stdio,
            capabilities: vec!["tools".to_owned()],
            timeout_ms: Some(1000),
        }
    }

    fn mock_client(
        handler: impl Fn(&JsonRpcRequest) -> Result<JsonRpcResponse, McpError> + Send + Sync + 'static,
    ) -> McpClient {
        McpClient::from_mock("test", config(), MockTransport::new(handler, 0)).unwrap()
    }

    #[tokio::test]
    async fn handshake_with_mock_server() {
        let client = mock_client(|request| {
            Ok(JsonRpcResponse::ok(
                request.id.clone(),
                serde_json::json!({
                    "protocolVersion": methods::PROTOCOL_VERSION,
                    "serverName": "mock",
                    "serverVersion": "0.1.0",
                    "capabilities": ["tools"],
                }),
            ))
        });
        let hello = client.initialize().await.unwrap();
        assert_eq!(hello.server_name, "mock");
        assert_eq!(client.status(), ServerStatus::Connected);
    }

    #[tokio::test]
    async fn tools_list_returns_parsed_schemas() {
        let client = mock_client(|request| {
            Ok(JsonRpcResponse::ok(
                request.id.clone(),
                serde_json::json!({
                    "tools": [
                        {"name": "read", "description": "read a file",
                         "inputSchema": {"type": "object"}},
                        {"name": "write", "description": "write a file",
                         "inputSchema": {"type": "object"}},
                    ]
                }),
            ))
        });
        let tools = client.list_tools().await.unwrap();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "read");
    }

    #[tokio::test]
    async fn tools_call_routes_through_policy_broker() {
        use policy_broker::{DefaultPolicyEngine, PolicyContext, PolicyDecision, PolicyEngine};
        let client = mock_client(|request| {
            Ok(JsonRpcResponse::ok(
                request.id.clone(),
                serde_json::json!({"content": [{"type": "text", "text": "ok"}], "isError": false}),
            ))
        });
        // Denied without authorization: transport never runs.
        let denied = client
            .call_tool("read", serde_json::json!({}), false)
            .await
            .unwrap_err();
        assert_eq!(denied.code, "denied");
        // Policy-broker verdict drives the authorized flag.
        let engine = DefaultPolicyEngine::new();
        let action =
            client.tool_action_request("task-1", "agent-1", "read", &serde_json::json!({}));
        // Fresh task has no evidence: write-like network egress gates behind
        // evidence (RequireEvidence), so the call stays denied.
        let gated = engine.evaluate(&action);
        assert_ne!(gated.decision, PolicyDecision::Allow);
        let still_denied = client
            .call_tool(
                "read",
                serde_json::json!({}),
                gated.decision == PolicyDecision::Allow,
            )
            .await
            .unwrap_err();
        assert_eq!(still_denied.code, "denied");
        // With recorded evidence the broker allows and the call proceeds.
        let allowed = engine.evaluate_with_context(
            &action,
            &PolicyContext {
                has_evidence: true,
                has_compensation_plan: false,
            },
        );
        assert_eq!(allowed.decision, PolicyDecision::Allow);
        let ok = client
            .call_tool("read", serde_json::json!({}), true)
            .await
            .unwrap();
        assert!(!ok.is_error);
    }

    #[tokio::test]
    async fn timeout_kills_server_process() {
        let slow = McpClient::from_mock(
            "slow",
            config(),
            MockTransport::new(
                |request| {
                    Ok(JsonRpcResponse::ok(
                        request.id.clone(),
                        serde_json::json!({"tools": []}),
                    ))
                },
                5000,
            ),
        )
        .unwrap();
        let err = slow.list_tools().await.unwrap_err();
        assert_eq!(err.code, "timeout");
        assert!(err.message.contains("killed"));
        assert_eq!(slow.status(), ServerStatus::Error);
        assert!(slow.last_error().is_some());
    }

    #[tokio::test]
    async fn resources_and_prompts_round_trip() {
        let client = mock_client(|request| match request.method.as_str() {
            "resources/list" => Ok(JsonRpcResponse::ok(
                request.id.clone(),
                serde_json::json!({"resources": [{"uri": "file:///a", "name": "a"}]}),
            )),
            "prompts/list" => Ok(JsonRpcResponse::ok(
                request.id.clone(),
                serde_json::json!({"prompts": [{"name": "review"}]}),
            )),
            _ => Ok(JsonRpcResponse::ok(
                request.id.clone(),
                serde_json::json!({}),
            )),
        });
        assert_eq!(client.list_resources().await.unwrap().len(), 1);
        assert_eq!(client.list_prompts().await.unwrap().len(), 1);
    }
}
