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
| `run_diagnostic`         | `runDiagnostic`                   | —               | `{ total, passed, checks[], ledger }`                                        |
| `verify_evidence_ledger` | `verifyEvidenceLedger`            | —               | `{ healthy, event_count, evidence_count, claim_count, detail, checked_at }`  |

Failures are structured `{ code, message }` — never raw panics across the boundary. `getAuditSummary` arrives with the audit UI.

Event names (`src/events.rs`) are string-identical to contracts `IpcEventName` so frontend listeners and backend emitters cannot drift.

## Security notes

- Capability `default` grants the main window **only**: zero filesystem, shell, HTTP, dialog, or notification permissions.
- Production CSP: `self` + Tauri `ipc:`/`asset:` schemes only. The dev CSP additionally allows `localhost:5173` + `ws:` for Vite HMR.
- `request_action` validates by construction: unknown capabilities fail deserialization and evaluate to `deny`.
- Evidence writes are append-only; the desktop state uses an in-memory SQLite database (no filesystem scope needed). File-backed persistence arrives later with explicit scope.
- Icons are generated placeholders (`tauri icon` from `icon-src.png`, since removed); replace with final brand art before release signing.
