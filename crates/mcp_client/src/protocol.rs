//! Model Context Protocol (MCP) wire types: JSON-RPC 2.0 envelopes plus the
//! MCP method set Talos speaks (`initialize`, `tools/list`, `tools/call`,
//! `resources/list`, `resources/read`, `prompts/list`, `prompts/get`).
//!
//! All types are `camelCase` JSON on the wire to match the MCP spec. Parsing
//! is strict: unknown or malformed envelopes are rejected, never executed.

/// JSON-RPC 2.0 request id (string or integer, per spec).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum RequestId {
    /// String id.
    Str(String),
    /// Integer id.
    Int(i64),
}

impl std::fmt::Display for RequestId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Str(value) => write!(f, "{value}"),
            Self::Int(value) => write!(f, "{value}"),
        }
    }
}

/// JSON-RPC 2.0 request envelope.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct JsonRpcRequest {
    /// Always `"2.0"`.
    pub jsonrpc: String,
    /// Request id (absent for notifications).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<RequestId>,
    /// Method name (`initialize`, `tools/list`, ...).
    pub method: String,
    /// Method parameters.
    #[serde(default)]
    pub params: serde_json::Value,
}

impl JsonRpcRequest {
    /// Build a notification (no id) or request for `method`.
    #[must_use]
    pub fn new(id: Option<RequestId>, method: &str, params: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_owned(),
            id,
            method: method.to_owned(),
            params,
        }
    }

    /// Validate the envelope version and method name.
    pub fn validate(&self) -> Result<(), McpError> {
        if self.jsonrpc != "2.0" {
            return Err(McpError::invalid("jsonrpc must be 2.0"));
        }
        if self.method.trim().is_empty() || self.method.len() > 128 {
            return Err(McpError::invalid("method must be 1..=128 chars"));
        }
        Ok(())
    }
}

/// JSON-RPC 2.0 error object.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct JsonRpcError {
    /// Numeric code.
    pub code: i64,
    /// Human-readable message.
    pub message: String,
    /// Optional extra data.
    #[serde(default)]
    pub data: serde_json::Value,
}

/// JSON-RPC 2.0 response envelope (exactly one of `result` / `error`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct JsonRpcResponse {
    /// Always `"2.0"`.
    pub jsonrpc: String,
    /// Mirrors the request id.
    #[serde(default)]
    pub id: Option<RequestId>,
    /// Success payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    /// Failure payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    /// Success response.
    #[must_use]
    pub fn ok(id: Option<RequestId>, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_owned(),
            id,
            result: Some(result),
            error: None,
        }
    }

    /// Error response.
    #[must_use]
    pub fn fail(id: Option<RequestId>, code: i64, message: String) -> Self {
        Self {
            jsonrpc: "2.0".to_owned(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message,
                data: serde_json::Value::Null,
            }),
        }
    }
}

/// Structured MCP failure.
#[derive(Debug, Clone, PartialEq, thiserror::Error, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpError {
    /// Stable code (`transport`, `timeout`, `protocol`, `invalid`, `denied`, ...).
    pub code: String,
    /// Human-readable message.
    pub message: String,
}

impl McpError {
    /// Build an error.
    #[must_use]
    pub fn new(code: &str, message: String) -> Self {
        Self {
            code: code.to_owned(),
            message,
        }
    }

    /// Transport-level failure (spawn, IO, SSE HTTP).
    #[must_use]
    pub fn transport(message: String) -> Self {
        Self::new("transport", message)
    }

    /// Response did not arrive before the deadline (server killed).
    #[must_use]
    pub fn timeout(message: String) -> Self {
        Self::new("timeout", message)
    }

    /// Malformed envelope or unexpected method.
    #[must_use]
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new("invalid", message.into())
    }

    /// Policy broker refused the call.
    #[must_use]
    pub fn denied(message: String) -> Self {
        Self::new("denied", message)
    }
}

impl std::fmt::Display for McpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "mcp {}: {}", self.code, self.message)
    }
}

/// MCP method names spoken by the client and server.
pub mod methods {
    /// Version handshake.
    pub const INITIALIZE: &str = "initialize";
    /// Discover tools.
    pub const TOOLS_LIST: &str = "tools/list";
    /// Invoke a tool.
    pub const TOOLS_CALL: &str = "tools/call";
    /// Discover resources.
    pub const RESOURCES_LIST: &str = "resources/list";
    /// Read a resource.
    pub const RESOURCES_READ: &str = "resources/read";
    /// Discover prompts.
    pub const PROMPTS_LIST: &str = "prompts/list";
    /// Fetch a prompt.
    pub const PROMPTS_GET: &str = "prompts/get";
    /// Protocol version Talos speaks.
    pub const PROTOCOL_VERSION: &str = "2024-11-05";
}

/// One tool advertised by an MCP server.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpTool {
    /// Tool name (`1..=128` chars).
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// JSON-Schema input shape.
    pub input_schema: serde_json::Value,
}

impl McpTool {
    /// Validate name and schema shape.
    pub fn validate(&self) -> Result<(), McpError> {
        if self.name.trim().is_empty() || self.name.len() > 128 {
            return Err(McpError::invalid("tool name must be 1..=128 chars"));
        }
        if !self.input_schema.is_object() {
            return Err(McpError::invalid("tool inputSchema must be an object"));
        }
        Ok(())
    }
}

/// One resource advertised by an MCP server.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpResource {
    /// Resource URI.
    pub uri: String,
    /// Display name.
    pub name: String,
    /// Optional description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Optional MIME type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

/// One prompt advertised by an MCP server.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpPrompt {
    /// Prompt name.
    pub name: String,
    /// Optional description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// `initialize` result: server identity plus protocol version.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    /// Negotiated protocol version.
    pub protocol_version: String,
    /// Server name.
    pub server_name: String,
    /// Server version.
    pub server_version: String,
    /// Declared capabilities.
    #[serde(default)]
    pub capabilities: Vec<String>,
}

/// `tools/call` result: content blocks plus error flag.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallToolResult {
    /// Content blocks (`[{ "type": "text", "text": ... }]`).
    #[serde(default)]
    pub content: Vec<serde_json::Value>,
    /// True when the tool itself failed.
    #[serde(default)]
    pub is_error: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_round_trips() {
        let request = JsonRpcRequest::new(
            Some(RequestId::Int(1)),
            methods::TOOLS_LIST,
            serde_json::json!({}),
        );
        assert!(request.validate().is_ok());
        let raw = serde_json::to_string(&request).unwrap();
        let back: JsonRpcRequest = serde_json::from_str(&raw).unwrap();
        assert_eq!(request, back);
    }

    #[test]
    fn rejects_bad_envelope() {
        let bad = JsonRpcRequest::new(None, "", serde_json::json!({}));
        assert!(bad.validate().is_err());
        let wrong = JsonRpcRequest {
            jsonrpc: "1.0".to_owned(),
            id: None,
            method: methods::TOOLS_LIST.to_owned(),
            params: serde_json::json!({}),
        };
        assert!(wrong.validate().is_err());
    }

    #[test]
    fn tool_validation_rejects_shapes() {
        let ok = McpTool {
            name: "read".to_owned(),
            description: "read".to_owned(),
            input_schema: serde_json::json!({"type": "object"}),
        };
        assert!(ok.validate().is_ok());
        let bad = McpTool {
            name: String::new(),
            description: "x".to_owned(),
            input_schema: serde_json::json!({"type": "object"}),
        };
        assert!(bad.validate().is_err());
        let bad_schema = McpTool {
            name: "read".to_owned(),
            description: "x".to_owned(),
            input_schema: serde_json::json!([]),
        };
        assert!(bad_schema.validate().is_err());
    }
}
