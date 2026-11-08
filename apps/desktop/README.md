# Talos Desktop — Tauri 2 shell

Secure native container for the Talos command center. React UI (`apps/ui` build output) inside, Rust policy broker + evidence store behind a least-privilege IPC bridge.

## Commands

```sh
pnpm dev:desktop        # Tauri dev: Vite server (:5173) + native window (needs display + WebView2)
pnpm build:desktop      # production bundle: builds UI, then Tauri installer (needs icons + bundler deps)
pnpm --filter desktop check   # environment + config report (tauri info)
```

From `apps/desktop/src-tauri` (with the MSVC env loaded):

```sh
cargo check --all-targets
cargo clippy --all-targets -- -D warnings
cargo test
```

## IPC surface

| Tauri command            | Contracts name (`IpcCommandName`) | Input           | Output                                                                       |
| ------------------------ | --------------------------------- | --------------- | ---------------------------------------------------------------------------- |
| `get_app_info`           | `getAppInfo`                      | —               | `{ name, version, environment }`                                             |
| `get_system_status`      | `getSystemStatus`                 | —               | `SystemStatus` (mirrors TS schema)                                           |
| `request_action`         | `requestAction`                   | raw JSON action | `PolicyEvaluation` (malformed input → deny verdict, never a transport error) |
| `run_diagnostic`         | `runDiagnostic`                   | —               | `{ total, passed, checks[], ledger, task_id, agent_id, evaluation }`         |
| `verify_evidence_ledger` | `verifyEvidenceLedger`            | —               | `{ healthy, event_count, evidence_count, claim_count, detail, checked_at }`  |

Failures are structured `{ code, message }` — never raw panics across the boundary.

Full surface (100 commands): task queue (`submit_task`, `cancel_task`, `retry_task`,
`list_tasks`, `get_task_state`, `delegate_task`, `get_task_tree`, `cancel_subtree`,
`preview_plan`, `get_planning_config`, `update_planning_config`), tools (`list_tools`,
`describe_tool`, `invoke_tool_dry_run`, `invoke_tool` — 34 built-ins incl. `fs.edit/create/delete/move`,
`code.run_tests/install_deps`, `git.status/diff/branch/commit`), MCP + plugins (`list_mcp_servers`,
`add_mcp_server`, `remove_mcp_server`, `list_mcp_tools`, `call_mcp_tool` — policy-gated with
`mcp_tool_call` evidence, `parse_instruction_file` for AGENTS.md/CLAUDE.md/SKILL.md/HEARTBEAT.md,
`list_plugins`, `approve_plugin` — ed25519 manifests, `plugin_approval` evidence;
events `mcpServerConnected/Disconnected`, `mcpToolCallCompleted`, `instructionFileParsed`,
`pluginApprovalRequested`), sandbox (`get_sandbox_status`,
`get_sandbox_profile_for_task` — OS profiles `read_only`/`workspace_write`/`danger_full_access`,
fail closed, `sandboxEnforced`/`sandboxViolation` events), workspaces, semantic memory (`search_memory`,
`get_index_status`, `rebuild_index`, `list_skills` with usage stats), GEPA skills (`get_skill`,
`validate_skill`, `publish_skill`, `deprecate_skill`, `archive_skill`, `get_skill_stats`,
`list_skill_versions`, `diff_skill_versions`, `trigger_skill_evaluation` — background loop,
events `skillCreated/Validated/Published/Deprecated/Archived/EvaluationCompleted`), computer-use, context, Tool-RAG (lexical/semantic/hybrid `mode`),
LLM gateway, assembly commands (with hybrid semantic segments), HITL approvals (`get_pending_approvals`,
`request_approval_review`, `grant_approval`, `deny_approval`, `modify_approval`, `batch_approve_low_risk`,
`set_task_mode`, `get_task_mode`, `get_plan_document`, `execute_compensation`, `fail_compensation_step` —
11 commands), computer sessions (`computer_session_scope`,
`computer_set_session_scope`), storage (`get_storage_info` with encryption status,
`create_backup`, `restore_backup`, `list_backups`, `get/set_onboarding_state` —
shared `app_state` flag, replayable from Settings), and updates (`check_for_updates`,
`get_update_status`, `download_update`, `install_update`, `verify_update_manifest`,
`get/set_update_channel` — ed25519-verified, staged rollouts, channels). See `src/lib.rs`
`invoke_handler!` and `@talos/contracts` `IpcCommandName` (single source of truth).

Real OS control is a compile-time + runtime opt-in: build the shell with
`--features real-os-control` (never in CI), enable the envelope, pass policy,
and set `TALOS_COMPUTER_REAL=1`. Default builds can only run the mock.
See `docs/COMPUTER_USE_REAL.md`.

HITL approvals: policy `require_human_review` opens a pending `ApprovalRequest`
(task moves to `waiting_policy`); grant resumes to `executing`, deny fails the
task, modify re-evaluates the patched action. Expiry sweeps run on every
`get_pending_approvals` (5-minute default). Events: `approvalRequested`
(full sheet record), `approvalGranted`, `approvalDenied`, `approvalExpired`,
`compensationExecuted`.

Coding workspaces: each coding task gets an isolated root (`<temp>/talos-workspaces/<task_id>`, git worktree when a repo is hinted). Tool execution for workspace tasks resolves the sandbox root to the workspace root with the coder allowlist (`process.exec` opted in per-invocation; manifest default stays disabled); all other tasks keep the shared tool root with execution disabled.

Event names (`src/events.rs`) are string-identical to contracts `IpcEventName` so frontend listeners and backend emitters cannot drift.

## Local control plane

`run_diagnostic` drives `agent_runtime::LocalOrchestrator`: create task → spawn
`MockEchoAgent` → propose read action → policy verdict → record evidence →
complete, then emits one Tauri event per stage (`taskCreated`, `agentSpawned`,
`taskUpdated`, `policyEvaluated`, `evidenceUpdated`). A 5s `heartbeat` keeps the
UI clock honest. The UI subscribes in `apps/ui/src/bridge/` (`useTalosSync`),
validates every payload with Zod, and funnels results into the Zustand stores —
the same path the real runtime will use later.

## Security notes

- Capability `default` grants the main window **only**: zero filesystem, shell, HTTP, dialog, or notification permissions.
- Production CSP: `self` + Tauri `ipc:`/`asset:` schemes only. The dev CSP additionally allows `localhost:5173` + `ws:` for Vite HMR.
- `request_action` validates by construction: unknown capabilities fail deserialization and evaluate to `deny`.
- Evidence writes are append-only; the desktop state uses a file-backed SQLite database under Tauri `app_data_dir` (`com.talos.desktop/talos.db`, `TALOS_DATA_DIR` override), encrypted at rest (ChaCha20-Poly1305 envelope, keychain-held 256-bit key, loud plaintext fallback only when the keychain is unreachable) with dirty-flag crash recovery and checkpoint resume. Backups are sealed with the same envelope.
- Updates are ed25519-signed: bundles install only with a valid signature against the pubkey pinned in `tauri.conf.json`; release signing is offline via `scripts/sign-release.mjs` (private key never in repo). The endpoint is a placeholder until the first release channel exists.
- Icons are generated placeholders (`tauri icon` from `icon-src.png`, since removed); replace with final brand art before release signing.
