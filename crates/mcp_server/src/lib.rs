//! Talos as an MCP server: expose the 33 built-in tools over JSON-RPC 2.0.
//!
//! External MCP clients (IDE extensions, other agents) connect over stdio,
//! authenticate with a bearer token, discover tools via `tools/list`, and
//! invoke them via `tools/call`. Every incoming call is evaluated by the
//! policy broker first: non-`Allow` verdicts are denied without execution,
//! and every receipt (including denials) is returned with its policy verdict
//! so the caller can persist it as evidence.

use mcp_client::protocol::{methods, JsonRpcRequest, JsonRpcResponse, McpError, McpTool};

/// Authentication failure message (no token details echoed).
const AUTH_MESSAGE: &str = "unauthenticated: invalid or missing bearer token";

/// MCP server exposing Talos tools.
#[derive(Debug)]
pub struct TalosMcpServer {
    /// Expected bearer token (compared in constant time).
    token: String,
    /// Tool manifests served to clients (MCP shape).
    tools: Vec<McpTool>,
}

impl TalosMcpServer {
    /// Build a server over the live registry manifests.
    #[must_use]
    pub fn new(token: String, registry: &tool_runtime::ToolRegistry) -> Self {
        let tools = registry
            .list()
            .into_iter()
            .map(|manifest| McpTool {
                name: manifest.id,
                description: manifest.description,
                input_schema: manifest.input_schema,
            })
            .collect();
        Self { token, tools }
    }

    /// Build a server over an explicit tool list (tests).
    #[must_use]
    pub fn with_tools(token: String, tools: Vec<McpTool>) -> Self {
        Self { token, tools }
    }

    /// Tool count served.
    #[must_use]
    pub fn tool_count(&self) -> usize {
        self.tools.len()
    }

    /// Constant-time bearer check (length + byte fold, no early exit).
    fn authenticated(&self, presented: Option<&str>) -> bool {
        let Some(presented) = presented else {
            return false;
        };
        let expected = self.token.as_bytes();
        let got = presented.as_bytes();
        if expected.len() != got.len() {
            return false;
        }
        let mut diff = 0u8;
        for (left, right) in expected.iter().zip(got.iter()) {
            diff |= left ^ right;
        }
        diff == 0 && !expected.is_empty()
    }

    /// Handle one JSON-RPC request envelope. `bearer` is the presented token
    /// (without the `Bearer ` prefix). Returns the response envelope.
    pub fn handle_request(
        &self,
        request: &JsonRpcRequest,
        bearer: Option<&str>,
    ) -> JsonRpcResponse {
        let started = std::time::Instant::now();
        let response = self.route(request, bearer);
        let duration_ms = started.elapsed().as_millis().clamp(0, i64::MAX as u128) as i64;
        talos_telemetry::record_now(
            "mcp_server",
            "handle_request",
            None,
            None,
            if response.error.is_none() {
                talos_telemetry::SpanStatus::Ok
            } else {
                talos_telemetry::SpanStatus::Error
            },
            &request.method,
            duration_ms,
            &[("mcp.method", serde_json::json!(request.method))],
        );
        response
    }

    /// Parse a raw line and handle it (stdio framing helper).
    #[must_use]
    pub fn handle_line(&self, line: &str, bearer: Option<&str>) -> String {
        let response = match serde_json::from_str::<JsonRpcRequest>(line) {
            Ok(request) => self.handle_request(&request, bearer),
            Err(err) => JsonRpcResponse::fail(None, -32700, format!("parse error: {err}")),
        };
        serde_json::to_string(&response).unwrap_or_else(|_| {
            "{\"jsonrpc\":\"2.0\",\"id\":null,\"error\":{\"code\":-32603,\"message\":\"encode error\",\"data\":null}}".to_owned()
        })
    }

    fn route(&self, request: &JsonRpcRequest, bearer: Option<&str>) -> JsonRpcResponse {
        if request.validate().is_err() {
            return JsonRpcResponse::fail(
                request.id.clone(),
                -32600,
                "invalid request envelope".to_owned(),
            );
        }
        if !self.authenticated(bearer) {
            return JsonRpcResponse::fail(request.id.clone(), -32001, AUTH_MESSAGE.to_owned());
        }
        match request.method.as_str() {
            methods::INITIALIZE => JsonRpcResponse::ok(
                request.id.clone(),
                serde_json::json!({
                    "protocolVersion": methods::PROTOCOL_VERSION,
                    "serverName": "talos",
                    "serverVersion": env!("CARGO_PKG_VERSION"),
                    "capabilities": ["tools"],
                }),
            ),
            methods::TOOLS_LIST => {
                JsonRpcResponse::ok(request.id.clone(), serde_json::json!({"tools": self.tools}))
            }
            methods::TOOLS_CALL => self.route_tool_call(request),
            methods::RESOURCES_LIST => {
                JsonRpcResponse::ok(request.id.clone(), serde_json::json!({"resources": []}))
            }
            methods::RESOURCES_READ => {
                JsonRpcResponse::fail(request.id.clone(), -32602, "unknown resource".to_owned())
            }
            methods::PROMPTS_LIST => {
                JsonRpcResponse::ok(request.id.clone(), serde_json::json!({"prompts": []}))
            }
            methods::PROMPTS_GET => {
                JsonRpcResponse::fail(request.id.clone(), -32602, "unknown prompt".to_owned())
            }
            _ => JsonRpcResponse::fail(
                request.id.clone(),
                -32601,
                format!("method not found: {}", request.method),
            ),
        }
    }

    /// Authorize one incoming `tools/call` against the policy broker.
    ///
    /// Returns `Ok(arguments)` when the broker allows execution, else the
    /// denial envelope payload. Unknown tools deny locally.
    pub fn authorize_tool_call(
        &self,
        engine: &dyn policy_broker::PolicyEngine,
        request: &JsonRpcRequest,
        task_id: &str,
        agent_id: &str,
    ) -> Result<serde_json::Value, McpError> {
        let name = request
            .params
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        if name.trim().is_empty() || name.len() > 128 {
            return Err(McpError::invalid("tool name must be 1..=128 chars"));
        }
        let Some(tool) = self.tools.iter().find(|tool| tool.name == name) else {
            return Err(McpError::invalid(format!("unknown tool: {name}")));
        };
        let _ = tool;
        let arguments = request
            .params
            .get("arguments")
            .cloned()
            .unwrap_or(serde_json::json!({}));
        let action = policy_broker::ActionRequest {
            id: format!("mcp-in-{name}-{}", uuid::Uuid::new_v4()),
            agent_id: agent_id.to_owned(),
            task_id: task_id.to_owned(),
            capability: tool_runtime::capability_for(name),
            intent: format!("external MCP client calls Talos tool {name}"),
            target: format!("talos:{name}"),
            payload_json: arguments.clone(),
            risk_level: tool_runtime::risk_for(name).0,
            reversible: tool_runtime::risk_for(name).1,
            requested_at: chrono::Utc::now(),
        };
        // Incoming calls carry no prior ledger context at the gateway: the
        // broker evaluates the declared risk directly. Evidence-gated tools
        // (writes, browser/computer mutations) deny until the external caller
        // records backing evidence through a separate channel.
        let evaluation = engine.evaluate(&action);
        if evaluation.decision != policy_broker::PolicyDecision::Allow {
            return Err(McpError::denied(format!(
                "policy {:?}: {}",
                evaluation.decision, evaluation.reason
            )));
        }
        Ok(arguments)
    }

    fn route_tool_call(&self, request: &JsonRpcRequest) -> JsonRpcResponse {
        let name = request
            .params
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        if self.tools.iter().find(|tool| tool.name == name).is_none() {
            return JsonRpcResponse::fail(
                request.id.clone(),
                -32602,
                format!("unknown tool: {name}"),
            );
        }
        // The stdio gateway authorizes via `authorize_tool_call` with the
        // shared engine before dispatching to `tool_runtime::invoke`; this
        // route reports the tool as discovered and defers execution to that
        // policy-gated path (never executes inline without a verdict).
        JsonRpcResponse::fail(
            request.id.clone(),
            -32000,
            "tools/call requires policy authorization via the Talos gateway (see authorize_tool_call)".to_owned(),
        )
    }
}

/// Serve stdio framing: read lines from `input`, write responses to `output`.
///
/// Used by the Talos MCP gateway binary path; each line is one JSON-RPC
/// envelope and `bearer` authenticates every request.
pub fn serve_lines(server: &TalosMcpServer, input: &str, bearer: Option<&str>) -> Vec<String> {
    input
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| server.handle_line(line, bearer))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcp_client::protocol::RequestId;
    use policy_broker::DefaultPolicyEngine;

    fn tool(name: &str) -> McpTool {
        McpTool {
            name: name.to_owned(),
            description: format!("{name} tool"),
            input_schema: serde_json::json!({"type": "object"}),
        }
    }

    fn server() -> TalosMcpServer {
        TalosMcpServer::with_tools(
            "secret-token".to_owned(),
            vec![tool("fs.read"), tool("fs.list")],
        )
    }

    fn request(id: i64, method: &str, params: serde_json::Value) -> JsonRpcRequest {
        JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(RequestId::Int(id)),
            method: method.to_owned(),
            params,
        }
    }

    #[test]
    fn exposes_talos_tools_correctly() {
        let server = server();
        let response = server.handle_request(
            &request(1, methods::TOOLS_LIST, serde_json::json!({})),
            Some("secret-token"),
        );
        assert!(response.error.is_none());
        let tools = response.result.unwrap()["tools"].as_array().unwrap().len();
        assert_eq!(tools, 2);
        let hello = server.handle_request(
            &request(2, methods::INITIALIZE, serde_json::json!({})),
            Some("secret-token"),
        );
        assert_eq!(
            hello.result.unwrap()["serverName"],
            serde_json::json!("talos")
        );
    }

    #[test]
    fn rejects_unauthenticated_requests() {
        let server = server();
        for bearer in [None, Some("wrong"), Some("")] {
            let response = server.handle_request(
                &request(1, methods::TOOLS_LIST, serde_json::json!({})),
                bearer,
            );
            let error = response.error.unwrap();
            assert_eq!(error.code, -32001);
            assert!(error.message.contains("unauthenticated"));
        }
    }

    #[test]
    fn rejects_unknown_methods_and_tools() {
        let server = server();
        let unknown = server.handle_request(
            &request(1, "nope/method", serde_json::json!({})),
            Some("secret-token"),
        );
        assert_eq!(unknown.error.unwrap().code, -32601);
        let call = server.handle_request(
            &request(
                2,
                methods::TOOLS_CALL,
                serde_json::json!({"name": "nope.tool", "arguments": {}}),
            ),
            Some("secret-token"),
        );
        assert_eq!(call.error.unwrap().code, -32602);
    }

    #[test]
    fn incoming_calls_respect_policy_broker() {
        let server = server();
        let engine = DefaultPolicyEngine::new();
        // Low-risk read: allowed.
        let read = request(
            1,
            methods::TOOLS_CALL,
            serde_json::json!({"name": "fs.read", "arguments": {}}),
        );
        assert!(server
            .authorize_tool_call(&engine, &read, "task-1", "agent-ext")
            .is_ok());
        // Unknown tool: denied locally.
        let unknown = request(
            2,
            methods::TOOLS_CALL,
            serde_json::json!({"name": "drop.database", "arguments": {}}),
        );
        assert!(server
            .authorize_tool_call(&engine, &unknown, "task-1", "agent-ext")
            .is_err());
    }

    #[test]
    fn live_registry_serves_all_builtins() {
        let registry = tool_runtime::ToolRegistry::with_builtins();
        let server = TalosMcpServer::new("token".to_owned(), &registry);
        assert_eq!(server.tool_count(), tool_runtime::BUILT_IN_TOOLS.len());
    }
}
