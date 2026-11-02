# Talos Batch 4 — OS-Level Sandbox: Evidence Report

Date: 2026-11-02 (UTC). Status: implemented; verification recorded below.

## 1. What was built

Real OS-level sandboxing for shell/process execution (`process.exec`,
`code.run_tests`, `code.install_deps`), replacing workspace-root-only
confinement. Three profiles, fail-closed, honest per-platform limits.

- **`crates/sandbox`** (new): `SandboxProfile`
  (`read_only` | `workspace_write` | `danger_full_access`), enforcement
  `SandboxConfig` (profile, workspace root, network flag + host allowlist,
  cpu/memory caps, timeout, allowed commands, danger approval id),
  `SandboxReceipt` (`profile`, `enforced`, `enforcementMethod`,
  `networkPolicy`, `violations`), `SandboxEnforcer` trait (`wrap_command`,
  post-spawn `PostSpawnGuard`, `enforcement_method`, `is_available`) with
  `enforce_or_deny` fail-closed gate (`sandbox_unavailable` — never
  unsandboxed). Backends:
  - Linux bubblewrap: ro toolchain binds, rw canonicalized workspace bind
    (tmpfs-before-bind ordering), `--unshare-net` default, `prlimit`
    RLIMIT_CPU (timeout-derived) / RLIMIT_AS / RLIMIT_NPROC=128,
    best-effort cgroup v2 clamp (`TALOS_CGROUP_ROOT` test hook).
  - macOS Seatbelt: pure per-config `.sb` generation (workspace-only
    writes, network deny/per-host allow), enforced via `sandbox-exec`.
  - Windows Job Objects (via `windows` crate; four documented `unsafe`
    blocks — the only `unsafe` in the workspace): ActiveProcessLimit,
    per-process/job memory caps, CPU-rate hard cap, post-spawn assignment
    with the guard held to reap.
    Test hooks: `TALOS_SANDBOX_FORCE_UNAVAILABLE`, `TALOS_BWRAP_BIN`,
    `TALOS_SANDBOX_EXEC_BIN`. 21 unit tests.
- **`tool_runtime` integration**: `SandboxConfig.profile` (default
  `workspace_write`) + `danger_approval_id`; `exec_live` and
  `spawn_bounded` gate before spawning (dry-run untouched); manifests
  carry `sandbox_profile` (`process.exec`/`code.*` = workspace_write);
  receipts embed the `SandboxReceipt` and persist as evidence; denials are
  `Denied` with code `sandbox_unavailable`. 8 new tests (read-only denial,
  backend-unavailable denial, danger approve/deny, receipts) + 4
  Linux-gated adversarial tests (write-outside-workspace blocked,
  network blocked, symlink canonicalized, fork bomb contained).
- **Desktop**: `sandbox.rs` (`get_sandbox_status`,
  `get_sandbox_profile_for_task`, danger HITL hold — `approval_required`
  with pending approval, grant then retry — `sandboxEnforced` /
  `sandboxViolation` emission); `tool_config_for` maps workspace →
  `workspace_write`, else `read_only`; 3 new tests. 70 IPC commands.
- **Contracts**: `sandbox.ts` (`SandboxProfile/Config/Receipt`,
  `SandboxStatus/Backend`, `SandboxProfileForTask`, both event payloads);
  `sandboxProfile` on `ToolManifest`; 2 new IPC commands + 2 events;
  stability lists updated. 4 new tests.
- **UI**: `getSandboxStatus`/`getSandboxProfileForTask` bridge,
  `sandboxEnforced`/`sandboxViolation` feed subscriptions, Computer-page
  `SandboxPanel` (platform, backends, fail-closed badge, unavailable
  banner, per-task profile lookup). 4 new tests.
- **CI**: rust-core installs `bubblewrap` so Linux escape tests run for real.
- **Docs**: `SECURITY.md` sandbox rewrite + platform/limitation matrix,
  `ARCHITECTURE.md` §2b-iii, ADR 0010, CHANGELOG, desktop README.

## 2. Requirement coverage

1. Profile types + manifest field ✅ 2. Platform backends (trait +
   bwrap/Seatbelt/Job Objects, availability probes, fail closed) ✅
2. tool_runtime gating (profile resolution, unavailable → DENY
   `sandbox_unavailable`, danger → HITL ApprovalSheet, receipt in
   evidence) ✅ 4. Resource limits (prlimit/RLIMIT_* + cgroup clamp,
   Job Object limits, violation recording; timeout machinery unchanged) ✅
3. Network egress (default deny via unshare-net/Seatbelt-deny/policy;
   allowlist on macOS rules, documented limits elsewhere) ✅
4. Escape tests (4 Linux-live + deterministic denials everywhere,
   symlink/canonicalization, env isolation by construction) ✅
5. Desktop IPC (`get_sandbox_status`, `get_sandbox_profile_for_task`;
   `sandboxViolation`, `sandboxEnforced`) ✅ 8. UI sandbox status
   (platform, backends, per-task profile, violation log via feed,
   unavailable banner) ✅ 9. Tests (Rust 33 new + TS 8 new; see §3) ✅
6. Docs + sync (this report; commit + PR + green CI) ✅

## 3. Verification results (all green)

- `cargo test -p sandbox`: **21 passed** (Windows job tests live here).
- `cargo test -p tool_runtime`: **74 passed** (incl. 12 new sandbox/escape).
- `cargo test -p talos-desktop`: **71 passed** (incl. 3 new sandbox).
- `cargo test -p agent_runtime`: 91 passed (unchanged).
- `cargo clippy --workspace --all-targets -- -D warnings`: clean (crates +
  desktop); `cargo fmt --check`: clean.
- `@talos/contracts` vitest: **81 passed** (incl. 4 new sandbox).
- UI vitest: computer/bridge suites 23 passed (incl. 4 new sandbox).
- `pnpm typecheck`: clean; UI `eslint --max-warnings=0`: clean (touched files).
- Pre-existing flake noted: `memory::status_and_rebuild_round_trip`
  intermittently fails in parallel runs (unrelated to this change).

## 4. Known limitations (documented, not silent)

- Windows has no filesystem namespace: confinement is cwd + symlink-escape
  rejection + job limits; containers required for full isolation.
- Per-host egress filtering exists on macOS Seatbelt only; Linux is
  unshare-or-host-network (allowlist at policy layer), Windows policy-only.
- Exact CPU-percent caps need a writable cgroup v2 (Linux); elsewhere
  RLIMIT_CPU/timeout bounds apply and the receipt says so.
- Seatbelt has no rlimit equivalent: macOS bounds are timeout-kill only.
