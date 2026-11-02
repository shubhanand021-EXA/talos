# mcp_client

Model Context Protocol (MCP) client: JSON-RPC 2.0 over stdio/SSE with
policy-gated tool calls.

```sh
cargo test -p mcp_client --lib
```

## Protocol (`protocol.rs`)

Strict JSON-RPC 2.0 envelopes (`JsonRpcRequest` / `JsonRpcResponse`) plus the
MCP method set: `initialize`, `tools/list`, `tools/call`, `resources/list`,
`resources/read`, `prompts/list`, `prompts/get` (protocol version
`2024-11-05`). Malformed envelopes are rejected, never executed.

## Config (`config.rs`)

`.talos/mcp.json` shape:

```json
{ "servers": { "name": { "command": "...", "args": [...],
  "env": {...}, "capabilities": [...] } } }
```

Stdio servers use `command` (+ `args`/`env`); SSE servers use
`"transport": "sse"` plus an `http(s)` `url`. Every server declares
`capabilities` (1–32) and a response timeout (default 30s, 100–300_000ms).

## Transports (`transport.rs`)

- `StdioTransport`: one spawned child per call (`kill_on_drop`), single
  line-delimited JSON exchange. On timeout the child is **killed and reaped**
  so a hung server can never wedge the caller.
- `SseTransport`: JSON-RPC over HTTP POST (accepts bare JSON or SSE `data:`
  frames), client timeout = server timeout.
- `MockTransport`: in-memory handler + artificial delay for policy/timeout
  tests.

## Client (`client.rs`)

`McpClient`: `initialize` handshake → `list_tools` / `list_resources` /
`read_resource` / `list_prompts` / `get_prompt` → `call_tool`.
`tools/call` is built as a `NetworkRequest` / Medium `ActionRequest` first
(`tool_action_request`): the caller evaluates it with the policy broker
**before any byte reaches the server**, and `call_tool(..., authorized:
false)` denies locally without touching the transport. `tool_call_evidence`
builds the hash-only evidence payload the caller persists.
