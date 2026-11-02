# mcp_server

Talos as an MCP server: expose the 33 built-in tools over JSON-RPC 2.0 so
external MCP clients (IDE extensions, other agents) can discover and call
them through the policy broker.

```sh
cargo test -p mcp_server --lib
```

## Model

- `TalosMcpServer::new(token, registry)` serves the live registry manifests
  as MCP tools; `with_tools` serves an explicit list (tests).
- Every request needs a bearer token (constant-time comparison; missing or
  wrong → `-32001 unauthenticated`, no token details echoed).
- `tools/list` returns tool schemas; `tools/call` is authorized via
  `authorize_tool_call(engine, ...)`, which maps the tool to its Talos
  capability/risk (`capability_for` / `risk_for`) and evaluates with the
  policy broker. Non-`Allow` verdicts deny without execution.
- Unknown methods → `-32601`; unknown tools/resources/prompts → `-32602`;
  malformed envelopes → `-32600`.
- `serve_lines` handles stdio framing (one envelope per line).

The gateway never executes inline: `tools/call` dispatch to
`tool_runtime::invoke` happens only after `authorize_tool_call` allows, and
the receipt (including the policy verdict) is returned so the caller can
persist it as evidence.
