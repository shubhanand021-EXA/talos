# Talos Security

> Current as of **v0.4.0 — Batch 4 "The Hardening"** (encrypted at rest, crash
> recovery, signed updater foundation, OS sandbox). This document supersedes
> all earlier Batch 1 notes: the store is file-backed and encrypted, command
> sandboxing is enforced at the OS layer, and crash recovery is implemented.

## Threat model

Talos is a **local-first desktop agent shell**: one user, one machine, no
Talos cloud, no remote telemetry, no third-party account. The adversary is
anyone or anything that can reach the machine or trick the agent:

| Threat                               | Example                                           | Primary control                                                    |
| ------------------------------------ | ------------------------------------------------- | ------------------------------------------------------------------ |
| Malicious or mistaken agent action   | `rm -rf ~`, exfil via `curl`, secrets in a prompt | Policy broker (default deny) + OS sandbox + HITL for high-risk     |
| Prompt injection (direct + indirect) | Poisoned web page, tool output, pasted text       | Evidence-gating, capability risk escalation, human review for High |
| Untrusted tool/browser content       | Malicious page, hostile repo, crafted artifact    | DOM-first browser safety, origin allowlists, hash-only evidence    |
| Computer-use overreach               | Clicks into settings, banking, password fields    | Safety envelope (dangerous zones, confidence floor, e-stop)        |
| Stolen disk / powered-off machine    | Laptop theft, backup theft, disk resale           | Encrypted at rest (keychain-held 256-bit key)                      |
| Running-process compromise           | Malware reading memory or the live DB file        | OS user isolation; **not** defended (see limitations)              |
| Supply-chain tampering of updates    | Forged update bundle, downgrade attack            | ed25519-signed updater, pinned pubkey                              |
| Secret leakage into logs/ledger      | API key in evidence, error message, report        | Keychain-only keys, hash-only evidence, secret-scanning swarm      |

Out of scope (non-goals): multi-user / team isolation on one machine, SSO/RBAC
(server-side concepts for a local app), protection against a malicious OS or
hardware attacker, and anonymity of network traffic (Ollama/OpenAI/Anthropic
calls and update checks necessarily reveal network metadata to those endpoints).

## Data flow

```text
User intent (UI)
    │  Tauri IPC, typed + Zod-validated, least-privilege capability (zero extra perms)
    ▼
Policy broker ── default deny ──▶ Allow / Deny / require_evidence /
                                   require_compensation / require_human_review
    │ Allow (+ evidence / compensation / approval recorded first)
    ▼
OS sandbox ── read_only | workspace_write | danger_full_access (HITL grant only)
    │ argv-exact spawn, cleared env, per-task cwd, process-group kill
    ▼
Tool / browser worker / computer adapter ── receipts (hashes + metadata, never raw secrets)
    │
    ▼
Evidence ledger (SQLite, encrypted at rest, hash-chained, WAL)
    │ verified transitions only; checkpoints every 5 tool calls
    ▼
UI (status, receipts, integrity pill, audit dashboard) + local spans/export only
```

Secrets flow orthogonally: **OS keychain → HTTP Authorization header only**.
`crates/talos_secrets` (`com.talos.desktop` service) holds provider keys and
the database key. Key material never enters IPC payloads, evidence rows,
events, logs, error messages, or audit reports. The audit swarm scans every
run for leaked secret patterns.

## Security boundaries

1. **Policy broker** (`crates/policy_broker`) — the _only_ authority for side
   effects. Default deny; write-like actions require recorded evidence;
   irreversible actions require a compensation plan; high-risk actions
   escalate to human review. No bypass paths: planning, delegation, and the
   gateway all re-enter the same gate at execution time.
2. **OS sandbox** (`crates/sandbox` + `tool_runtime`) — three profiles,
   enforced at the OS layer with fail-closed semantics:
   - `read_only` — no process execution at all (`process.exec` / `code.*`
     denied before spawning; reads only).
   - `workspace_write` — execution inside a platform sandbox: Linux
     bubblewrap namespaces (read-only toolchain binds, read-write workspace
     bind, `--unshare-net` by default, `prlimit` CPU/memory/process-count
     bounds, best-effort cgroup v2 clamp) · macOS Seatbelt (`sandbox-exec`
     profile: writes only under the workspace, network denied unless
     allowlisted) · Windows Job Objects (CPU/memory/process-count limits +
     cwd confinement).
   - `danger_full_access` — full user access with **no** OS confinement;
     reachable only through an explicit human approval in the ApprovalSheet
     (single-shot grant, then retry).
   - Fail closed: when no OS backend can be established
     (`sandbox_unavailable` — bubblewrap/Seatbelt missing, Job Object
     creation failed), execution is denied rather than running unsandboxed.
   - Every exec receipt carries a `SandboxReceipt` (`profile`, `enforced`,
     `enforcementMethod`, `networkPolicy`, `violations`), persisted as
     evidence; `sandboxEnforced` / `sandboxViolation` events drive the UI
     banner and feed.
3. **Keychain** (`crates/talos_secrets`) — OS credential store holds the
   256-bit database key (`com.talos.desktop` /
   `database.encryption_key`) and provider API keys
   (`provider.{id}.api_key`). Hermetic mock backend exists for tests only;
   production always uses the OS backend.
4. **Encryption at rest** (`crates/evidence_store::crypto`, Batch 4) —
   ChaCha20-Poly1305 file envelope (magic `TALOSENV` + version + random
   12-byte nonce per encryption, AEAD over header). SQLite runs against a
   decrypted working copy; the canonical `talos.db` on disk is always
   ciphertext while encrypted, and backups (`VACUUM INTO` snapshots) are
   sealed with the same envelope. Rationale: `sqlx` 0.8 has no SQLCipher
   feature, so page-level encryption is unavailable; the envelope is the
   documented equivalent.
5. **Crash recovery** (`crates/evidence_store::recovery` + `checkpoints`) —
   a `shutdown_state` flag is set dirty on every startup and cleared only on
   orderly shutdown; a dirty flag on boot proves the previous process died
   mid-flight. `executing` tasks with a checkpoint resume (`queued` +
   `resumed_from_checkpoint` + `task_resumed` ledger event); without one
   they fail closed (`crash_recovery_failed`). Checkpoints (task status,
   chain head, bounded workspace digest, tool-receipt count) are ledgered as
   `task_checkpoint` evidence every 5 tool calls. Orphan working files from a
   crash are adopted (latest state wins); stale non-database files are
   quarantined, never deleted.
6. **Signed updater** (`apps/desktop/src-tauri/src/updater.rs` +
   `scripts/sign-release.mjs`, Batch 4) — staged-rollout endpoint serves
   version manifests; bundles install only with a valid ed25519 signature
   against the pubkey pinned in `tauri.conf.json`
   (`plugins.updater.pubkey`). IPC: `check_for_updates` (reports, never
   downloads), `install_update` (staged-or-fresh install, then restart), `get_update_status` (channel + rollout cohort + staged bytes), `download_update` (verified stage to temp), `verify_update_manifest` (offline feed validation), and `get/set_update_channel` (Batch 4 distribution layer).
   The private key must never enter the repo; CI verifies the sign/verify
   round-trip, tamper rejection, wrong-key rejection, and cross-implementation compatibility (Node signer, Rust minisign-verify, the same semantics the Tauri updater enforces).
7. **Signed installers** (`tauri.conf.json` bundle config, Batch 4):
   Windows `.msi` + NSIS `.exe`, macOS `.dmg`, Linux `.AppImage` + `.deb`;
   all bundles ship LICENSE + README + icons and (release builds) the
   `talos` CLI sidecar. Dev builds are unsigned (SmartScreen/Gatekeeper
   warn, expected); production uses a purchased code-signing certificate
   on Windows (thumbprint injected at release time; `osslsigncode` for
   cross-signing) and an Apple Developer identity + notarization on macOS
   (ad-hoc only for dev). No OS signing on Linux (updater-signature
   integrity instead). Installer size target: under 50 MB.
8. **Tauri least privilege** (`capabilities/default.json`) — the main window
   gets zero filesystem, shell, HTTP, dialog, or notification permissions.
   Production CSP allows only `self` + Tauri `ipc:`/`asset:` schemes. Update
   checks go through the backend updater plugin, not frontend fetch.
9. **Append-only ledger** (`crates/evidence_store`) — hash-chained
   `event_log` + evidence chain (SHA-256), claim lifecycle with explicit
   transitions, compensation plans, tamper-detection tests. Status changes go
   through repository transition functions — never ad-hoc edits.
10. **No secrets in repo** — the audit swarm scans for API keys, private keys,
    and tokens on every run (pattern definitions and test fixtures excluded
    by design; production paths scanned).

## Supported platforms and sandbox coverage

| Platform              | FS isolation                                                                                       | Network egress                                                                               | CPU / memory / process limits                                                                           |
| --------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| Linux (bubblewrap)    | namespaces: ro toolchain binds, rw workspace bind, tmpfs /tmp                                      | `--unshare-net` default; host-network only with policy allowlist (bwrap cannot filter hosts) | `prlimit` RLIMIT_CPU/AS/NPROC + best-effort cgroup v2 clamp (exact cpu-percent needs a writable cgroup) |
| macOS (Seatbelt)      | profile: writes only under workspace                                                               | `deny network*` default; per-host allow rules when configured                                | timeout kill only (Seatbelt has no rlimit equivalent)                                                   |
| Windows (Job Objects) | cwd confinement + symlink-escape rejection (kernel namespaces require containers — documented gap) | no per-process egress control; enforced at the policy layer                                  | Job Object CPU rate / memory / ActiveProcessLimit                                                       |

- Sandboxed children run argv-exact (no shell), with a cleared environment
  (only `PATH`/`SystemRoot`/toolchain/temp vars cross), a per-task cwd,
  process-group/tree kill, and `kill_on_drop` reaping.
- Exact CPU-percent caps on Linux require a writable cgroup v2 hierarchy;
  without one, `RLIMIT_CPU` (timeout-derived) bounds CPU time and the receipt
  records the fallback.
- The browser worker (Node.js subprocess) keeps its existing 30s kill-timeout
  supervision and is outside the shell-execution sandbox boundary (managed
  worker, not arbitrary shell).

## Known limitations (honest, specific)

1. **Running-process reads are not defended.** While the app runs, the
   decrypted working copy (`talos.work.db`, mode `0600` on Unix) and process
   memory hold plaintext. The envelope protects data at rest (stolen disk,
   powered-off machine, backups) — not against malware already running as
   the user. A crash can leave the working copy behind; the next startup
   adopts it for recovery, then re-encrypts.
2. **Secure wipe is best-effort.** Working-copy deletion overwrites with
   zeros, but SSD wear-leveling means remnant blocks may persist. Full-disk
   encryption remains the recommended complement.
3. **Keychain-unavailable fallback is loud but plaintext.** If the OS
   keychain is unreachable, the database runs unencrypted with a startup
   warning, a `encryption_unavailable` ledger event, and an UNENCRYPTED
   badge in Settings → Storage. Never silent.
4. **Resume replays from the checkpoint, not from the exact instruction.**
   Non-idempotent side effects may repeat after a crash; the checkpoint
   (chain head, receipt count, workspace digest) tells the operator exactly
   where execution stopped.
5. **Updater endpoint is a placeholder** (`https://updates.talos.dev/...`)
   until the first release channel exists. The signing infrastructure
   (keypair flow, `.sig` format, verification test) is real and tested.
6. **Windows network sandboxing is policy-layer only** (no per-process
   egress control); kernel-namespace isolation needs containers.
7. **The echo agent auto-verifies its own diagnostic evidence** (mock trust —
   a real verifier arrives later).
8. **Icons are generated placeholders**; replace before release signing.

## Responsible disclosure

Do **not** report suspected vulnerabilities in public issues. Use the private
channel in [SECURITY.md](../SECURITY.md) (GitHub
`Security → Report a vulnerability`). Open a regular issue with the audit
report attached (`docs/reports/audit-latest.md`) only for non-security
findings. Do not post suspected secrets — rotate them first.
