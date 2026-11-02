//! MCP client: JSON-RPC 2.0 over stdio/SSE with policy-gated tool calls.
//!
//! A Talos operator configures servers in `.talos/mcp.json`; every
//! `tools/call` is evaluated by the policy broker before any byte reaches
//! the server, recorded in the evidence ledger, and bounded by a response
//! timeout (default 30s, stdio children killed on expiry).

pub mod client;
pub mod config;
pub mod protocol;
pub mod transport;

pub use client::{tool_call_evidence, McpClient, ServerStatus};
pub use config::{McpConfigFile, McpServerConfig, TransportKind, DEFAULT_TIMEOUT_MS};
pub use protocol::{
    methods, CallToolResult, InitializeResult, JsonRpcError, JsonRpcRequest, JsonRpcResponse,
    McpError, McpPrompt, McpResource, McpTool, RequestId,
};
pub use transport::{MockTransport, SseTransport, StdioTransport};
