# ADR 0010: OS-level sandbox architecture for shell/process execution

Date: 2026-11-02
Status: accepted

## Context

Audit reports converge: a working directory plus a cleared environment is
not a security sandbox (ChatGPT P1-safety, Manus Critical, Meta §3.6
Codex). The policy broker gates _whether_ a command runs but cannot
contain _what it does_. Open Interpreter documents the target end-state:
namespace/Seatbelt sandboxing with read-only, workspace-write, and
dangerous full-access modes.

## Decision

1. **New `crates/sandbox` crate** (no policy, no I/O beyond spawning):
   `SandboxProfile` (`read_only` | `workspace_write` |
   `danger_full_access`), enforcement `SandboxConfig`, `SandboxReceipt`,
   and a `SandboxEnforcer` trait with platform backends — Linux bubblewrap
   namespaces + `prlimit` (+ best-effort cgroup v2 clamp), macOS Seatbelt
   profiles generated per config, Windows Job Objects (CPU/memory/process
   limits).
2. **Fail closed, always.** `enforce_or_deny` denies with
   `sandbox_unavailable` when no backend can be established; `read_only`
   denies before spawning; `danger_full_access` requires a granted human
   approval id at validation time. Escape/denial paths are unit-tested on
   every platform; live adversarial tests run on Linux CI (bubblewrap
   installed by the workflow).
3. **Honest limitations, recorded.** Filesystem namespaces on Windows
   require containers; per-host egress filtering exists on macOS Seatbelt
   only (Linux = unshare or host-network + policy allowlist, Windows =
   policy layer). Every gap is stated in the enforcement method string,
   the receipt, `docs/SECURITY.md`, and the UI banner — never silently
   widened.
4. **Receipts flow with execution.** The `SandboxReceipt` rides in tool
   output JSON and is persisted as evidence; `sandboxEnforced` /
   `sandboxViolation` IPC events drive the feed and the Computer-page
   status panel.
5. **Win32 FFI is contained.** The `windows` crate bindings are `unsafe
fn`, so `crates/sandbox` carries a documented per-crate lint override
   (four `unsafe` blocks with SAFETY comments for job create/configure/
   assign/close). Every other crate keeps `unsafe_code = "forbid"`.

## Consequences

- `process.exec` / `code.*` cannot run without an OS backend except via an
  explicit, audited human danger grant.
- CI installs bubblewrap so escape tests execute for real on Linux;
  platform-gated tests cover macOS/Windows specifics.
- Exact CPU-percent caps need a writable cgroup v2 (Linux) and remain a
  documented gap elsewhere; `RLIMIT_CPU`/timeout bounds apply everywhere
  a backend exists.
