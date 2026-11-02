# ADR 0011: MCP client/server + open-standards interoperability

Date: 2026-11-02
Status: accepted

## Context

Audit reports converge: Talos must meet users where they already work
(Manus §1.5.4: versioned MCP client/server contract, plugin/skill
packaging, lifecycle hooks, provenance, pinning, permission review,
reload/rollback; ChatGPT §16: MCP client/server, Agent Skills format,
AGENTS.md, ACP, plugin SDK; Meta §4: MCP gateway, discovery, per-agent
tool brokering; Open Interpreter / Codex / Claude Code: MCP, skills,
hooks, permissions, AGENTS.md, ACP). Talos has no MCP implementation, no
instruction-file parsers, and no ACP compatibility; `tool_runtime` routes
33 internal tools but speaks no external protocol.

## Decision

1. **New `crates/mcp_client` crate** (JSON-RPC 2.0 over stdio/SSE):
   `initialize` handshake, `tools/list`, `tools/call`, `resources/list`,
   `resources/read`, `prompts/list`, `prompts/get`. Config file
   `.talos/mcp.json` (`{ "servers": { name: { command, args, env,
capabilities } } }`, SSE via `transport: "sse"` + `url`). Every server
   declares capabilities; every `tools/call` is built as a
   `NetworkRequest`/Medium `ActionRequest` and evaluated by the policy
   broker **before any byte reaches the server**; calls are recorded in the
   evidence ledger (`mcp_tool_call`, output hashes only). Response timeout
   defaults to 30s; stdio children are killed and reaped on expiry.
2. **New `crates/mcp_server` crate** (Talos as MCP server): serves the 33
   built-in manifests over stdio framing, requires a bearer token
   (constant-time check, `-32001` otherwise), and authorizes every incoming
   `tools/call` with the policy broker (`capability_for`/`risk_for` mapping;
   non-`Allow` denies without execution). This lets IDE extensions and other
   tools use Talos as a tool provider without bypassing policy.
3. **New `crates/instruction_parser` crate** (deterministic, bounded):
   AGENTS.md (global + per-agent scopes, tool allow/deny, model prefs),
   CLAUDE.md (6-level hierarchy, build/convention/gotcha/constraint
   categories), SKILL.md (frontmatter + steps → `skills`-table payload,
   born quarantined), HEARTBEAT.md (natural-language schedules →
   `ScheduledTask` definitions; execution in Batch 5).
4. **Context assembly integration** (`context_assembler::instructions`):
   AGENTS.md/CLAUDE.md merge into `safety_rules` (verbatim, never
   compacted); SKILL.md procedures become `protected + safety_critical`
   tool-layer segments so TypeCompact preserves them byte-identical;
   malformed files skip with a recorded reason, never fatal.
5. **Plugin trust foundation** (`instruction_parser::plugin` + desktop
   `mcp.rs`): `PluginManifest` (name, version, author, capabilities,
   permissions, source_url, ed25519 signature); new servers/skills render a
   permission review, stay `pending` until explicitly approved, and record
   the verdict as `plugin_approval` evidence. Signed manifests verify
   against a 32-byte ed25519 key; bad signatures fail closed; unsigned
   plugins enable only on explicit approval.
6. **Desktop IPC (8 commands) + 5 events**: `list/add/remove_mcp_servers`,
   `list_mcp_tools`, `call_mcp_tool` (policy-gated receipt, allowed or
   denied — never a transport call on deny), `parse_instruction_file`,
   `list_plugins`, `approve_plugin`; events `mcpServerConnected`,
   `mcpServerDisconnected`, `mcpToolCallCompleted`, `instructionFileParsed`,
   `pluginApprovalRequested`. UI: `/mcp` route (servers, discovery, call
   form, instruction viewer, plugin/skill browser, permission-review
   modal); contracts `mcp.ts` is the single source of truth.

## Consequences

- External MCP servers are untrusted transport: policy + evidence +
  timeouts contain them; a hung server is killed, never retried silently.
- Instruction files are safety-critical input: verbatim through assembly
  and TypeCompact, with deny-lists folded into task constraints.
- ACP wire compatibility is **not** in this batch (foundation only:
  JSON-RPC framing + tool routing the ACP transport can reuse); tracked as
  future work alongside HEARTBEAT.md execution (Batch 5).
- 80 IPC commands total (was 72); `IpcCommandName`/`IpcEventName` stability
  tests updated.
