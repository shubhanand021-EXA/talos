# Talos Security Notes

Threat model: a local-first desktop agent shell. No cloud accounts, no remote telemetry. The agent plans and executes shell commands, test suites, and dependency installs on the user's machine — so process execution is the highest-risk surface and is contained by OS-level sandboxing (below), never by policy checks alone.

## Enforcement points

- **Policy broker is the only authority** for side effects (`crates/policy_broker`). Default deny; write-like actions require recorded evidence; irreversible actions require a compensation plan (`PolicyContext` hooks, no bypass paths).
- **OS sandbox for process execution** (`crates/sandbox` + `crates/tool_runtime`). Three profiles, enforced at the OS level with fail-closed semantics:
  - `read_only` — no process execution at all (`process.exec` / `code.*` are denied before spawning; reads only).
  - `workspace_write` — execution inside a platform sandbox: Linux bubblewrap namespaces (read-only toolchain binds, read-write workspace bind, `--unshare-net` by default, `prlimit` CPU/memory/process-count bounds, best-effort cgroup v2 clamp) · macOS Seatbelt (`sandbox-exec` profile: writes only under the workspace, network denied unless allowlisted) · Windows Job Objects (CPU/memory/process-count limits + cwd confinement).
  - `danger_full_access` — full user access with **no** OS confinement; reachable only through an explicit human approval in the ApprovalSheet (single-shot grant, then retry).
  - Fail closed: when no OS backend can be established (`sandbox_unavailable` — bubblewrap/Seatbelt missing, Job Object creation failed), execution is denied rather than running unsandboxed. The `SandboxReceipt` (`profile`, `enforced`, `enforcementMethod`, `networkPolicy`, `violations`) rides in every exec receipt and is persisted as evidence; `sandboxEnforced` / `sandboxViolation` events drive the UI banner and feed.
- **Tauri least privilege** (`apps/desktop/src-tauri/capabilities/default.json`): the main window gets zero filesystem, shell, HTTP, dialog, or notification permissions. Production CSP allows only `self` + Tauri `ipc:`/`asset:` schemes.
- **Append-only ledger** (`crates/evidence_store`): hash-chained `event_log` + evidence chain (SHA-256), claim lifecycle with explicit transitions, tamper-detection tests. Status changes go through repository transition functions — never ad-hoc edits.
- **No secrets in repo**: the audit swarm scans for API keys, private keys, and tokens on every run (pattern definitions and test fixtures are excluded by design; production paths are scanned).

## Platform coverage and honest limitations

| Platform              | FS isolation                                                                                       | Network egress                                                                               | CPU / memory / process limits                                                                           |
| --------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| Linux (bubblewrap)    | namespaces: ro toolchain binds, rw workspace bind, tmpfs /tmp                                      | `--unshare-net` default; host-network only with policy allowlist (bwrap cannot filter hosts) | `prlimit` RLIMIT_CPU/AS/NPROC + best-effort cgroup v2 clamp (exact cpu-percent needs a writable cgroup) |
| macOS (Seatbelt)      | profile: writes only under workspace                                                               | `deny network*` default; per-host allow rules when configured                                | timeout kill only (Seatbelt has no rlimit equivalent)                                                   |
| Windows (Job Objects) | cwd confinement + symlink-escape rejection (kernel namespaces require containers — documented gap) | no per-process egress control; enforced at the policy layer                                  | Job Object CPU rate / memory / ActiveProcessLimit                                                       |

- Sandboxed children run argv-exact (no shell), with a cleared environment (only `PATH`/`SystemRoot`/toolchain/temp vars cross), a per-task cwd, process-group/tree kill, and `kill_on_drop` reaping.
- Exact CPU-percent caps on Linux require a writable cgroup v2 hierarchy; without one, `RLIMIT_CPU` (timeout-derived) bounds CPU time and the receipt records the fallback.
- The browser worker (Node.js subprocess) keeps its existing 30s kill-timeout supervision and is outside the shell-execution sandbox boundary (managed worker, not arbitrary shell).

## What is NOT yet secured

- The SQLite store is file-backed (`<app_data>/talos.db`, WAL, idempotent
  migrations, startup integrity check + crash recovery, `VACUUM INTO`
  backups with newest-5 retention) — but there is **no at-rest encryption
  yet**; anyone with filesystem access can read `talos.db`. Full-disk
  encryption is the current mitigation; encrypted-at-rest storage (SQLCipher
  or equivalent) is future work.
- The echo agent auto-verifies its own diagnostic evidence (mock trust — a real verifier arrives later).
- Icons are generated placeholders; replace before release signing.

## Reporting issues

Do **not** report suspected vulnerabilities in public issues. Use the private channel in [SECURITY.md](../SECURITY.md) (`security@talos.dev` placeholder or `Security → Report a vulnerability`). Open a regular issue with the audit report attached (`docs/reports/audit-latest.md`) only for non-security findings. Do not post suspected secrets — rotate them first.
