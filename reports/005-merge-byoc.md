# Report 005 — Verify-and-merge: BYOC remote runners (PR #27)

- Date: 2026-11-12 (UTC)
- Agent: OpenCode (sole engineer)
- Batch: 1 / Prompt: 005 (review-and-merge; work existed on PR #27, never rebuilt)
- Read order followed: `AGENTS.md` → `VISION.md` → `reports/001-*`, `002-*`,
  `003-*`, `004-*` → `docs/CI.md` (required set read, never hardcoded from memory)
- Branch strategy: `gh pr checkout 27` → rebase onto post-merge `main`
  (`8a17456`) → one in-scope fix commit → push → CI green → `--merge`
  (history preserved per prompt; branch auto-deleted)

## 1. Rebase log

- `gh pr checkout 27` → local `feat/byoc-remote-runners` tracking
  `origin/feat/byoc-remote-runners` (8 commits on old base `04c80aa`).
- `git fetch origin` → `git rebase origin/main`: all 8 commits replayed
  **cleanly, zero conflicts** (rebase output: `Successfully rebased`).
  No license conflicts arose: the branch predates the Apache-2.0 relicense
  but adds no `license = "MIT"` lines — verified post-rebase by grep
  (zero MIT hits in `*.toml`/`package.json`; `LICENSE` still full Apache-2.0).
- `pnpm-lock.yaml` / `Cargo.lock`: byte-identical to `main` (empty
  `git diff origin/main...HEAD --stat` on both lockfiles) — no dependency
  changes in the branch.
- In-scope fix (1 commit, +7/−3):
  `fix(remote): fail closed when assigned runner is unresolvable
[Prompt-005]` (`b0e99aa`) — see §3 B3.
- Push required `--force-with-lease` (rebased feature branch; `main`
  untouched until merge — no force-push to `main`, ever).

## 2. Architecture summary

Target abstraction (`crates/tool_runtime/src/adapters.rs:34-52`):
`ExecutionTarget::{Local (default), Remote { runner_id }}` on
`SandboxConfig`; `remote_runner: Option<Arc<RemoteRunner>>` resolved from
`.talos/runners.json` by id. Desktop defaults every config to `Local`
(`commands.rs:1091,1112,1165,2429`); `apply_task_runner` flips to `Remote`
only for tasks with a validated assignment.

Transport (`crates/remote_runner`): SSH = argv-exact `ssh`/`tar` spawns
(`-i` key path, `BatchMode=yes`, `-a` no agent forwarding, `-x` no X11,
connect timeout, tar-pipe workspace sync, 32 MiB sync-back cap with
truncate-marker); Docker = `docker run --rm --network none` (default
air-gapped), exactly one `-v stage:/work` bind-mount, per-invocation
ephemeral container, resource caps. `SystemRunner` (`runner.rs:75-170`):
`env_clear` + minimal env, `kill_on_drop`, explicit kill+reap on timeout,
5s bounded post-exit drain. `MockRunner` for hermetic tests.

Credential handling: SSH key = user-owned file referenced by _path only_
in `runners.json` (0600 user file read by the ssh client; passphrases
unsupported by design; key material never enters Talos memory, logs, or
receipts). Channel bot tokens (Telegram/Discord, same branch) are
keychain-backed via `talos_secrets` (`crates/channels/src/registry.rs:257-280`;
`save_round_trip_is_secret_free` test). Status views never carry key
material (`RunnerStatusView`; `list_runners`).

Evidence routing: every remote result returns `RemoteOutput` with
`sha256:` over `stdout\nstderr` (`runner.rs:252-262`); the executor
persists a hash-chained `tool_receipt` with `remote_runner_id` +
`output_sha256` + `LIVE_REMOTE` fidelity (`executor.rs:634-690`);
timeouts/unreachable map to `Failed` + `timed_out`/`remote_unreachable`
receipts (`ssh.rs:124-136`, `docker.rs:91-103`, `executor.rs:855-872`).
The remote side is never the source of truth — hashes are computed
locally from returned bytes and ledgered locally.

Riding-along Batch-5 commits (reviewed at architecture + gate level, not
rebuilt): gateway daemon (loopback-only `TcpListener::bind(LOCALHOST,·)`,
`talos_gateway/src/server.rs:59`, loopback test at `:648`), HEARTBEAT
scheduler, Telegram/Discord channels, browser profiles + HITL bridge,
computer-use VM isolation (loopback VNC only, `vm.rs:496`).

## 3. Constitution review table B1–B7

| #   | Item                                                 | Verdict                   | Evidence (file:line)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| --- | ---------------------------------------------------- | ------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| B1  | No bypass: remote passes default-deny broker         | PASS                      | `executor.rs:441-593` — policy evaluates + deny persists receipt _before_ `run_adapter`; `run_adapter:962-984` — Remote only via gate; tests `remote_policy_denied_never_touches_the_wire` (`executor.rs:3970`, mock call_count 0)                                                                                                                                                                                                                                                                                                            |
| B2  | Local hash-chained receipt for every remote call     | PASS                      | `persist_receipt` `executor.rs:175-200`; receipt carries `remote_runner_id` + `output_sha256` (`:682-684`), `evidence_id` asserted in tests (`:4054`, `:4087-4090`); hash computed locally (`runner.rs:252`)                                                                                                                                                                                                                                                                                                                                  |
| B3  | Fail-closed, never silent local fallback             | PASS (after in-scope fix) | Unlinked runner → Denied `remote_unlinked` (`executor.rs:721-735`, test `:4126`); unreachable → Failed `remote_unreachable` + Mock fidelity (`:864-872`, test `:4058`); non-exec → Denied `remote_unsupported` (`:968-980`, test `:4099`); **fix**: unresolvable _assignment_ at invoke time previously fell back to Local with only a stderr log — now sets `Remote` + `None` so it denies with a ledgered `remote_unlinked` receipt (`commands.rs:1122-1149`, commit `b0e99aa`)                                                             |
| B4  | Secrets: keychain/path discipline; redaction by test | PASS (rationale recorded) | SSH key material never in-process (ssh client reads file); config holds path only (`config.rs:1-5`); `env_clear` + `HOST_PASSTHROUGH` allowlist (`runner.rs:105-112`, `adapters.rs:63-86`); argv-shape test (`ssh.rs:256-268`); no-password-prompts assertion; channel tokens keychain-backed (`channels/registry.rs:257-280`); telemetry redaction tests (`talos_telemetry/privacy.rs:137`). Residual: SSH uses user-owned key files rather than the OS keychain (key never enters Talos) — substance-compliant; keychain-via-agent is WP-F1 |
| B5  | Sandbox/receipt semantics; fidelity ladder intact    | PASS                      | `LiveRemote` ranked Live < LiveRemote < Verified (`talos_common/fidelity.rs:27-42`); min-parent rule + oversell-is-error tests (`:136-192`); attempted-but-unproven remote maps to Mock, never Live (`executor.rs:673-680` + test `:4091-4095`); per-target isolation (SSH dir jail, Docker single-mount `--rm --network none`); gateway loopback-only (`server.rs:59,648`)                                                                                                                                                                   |
| B6  | Tests for deny/allow/timeout/unreachable             | PASS                      | `remote_runner`: 17 tests (argv isolation, timeouts, refuse-missing-workspace, hash stability); `tool_runtime`: 5 remote tests (`executor.rs:3970-4159` deny/allow/timeout/unreachable/unlinked); full workspace suite green (§4)                                                                                                                                                                                                                                                                                                             |
| B7  | Pool lifecycle uses bounded drain contract           | PASS (vacuous)            | grep over `crates/remote_runner` for `sqlx\|Pool\|drain_pool\|.close()` returns zero matches — the crate introduces no pools, so the Prompt-002 contract is inapplicable; no unbounded `close()` anywhere in the branch's new code                                                                                                                                                                                                                                                                                                            |

Zero unresolved P0 gaps. No BLOCKED report needed.

## 4. Test results

Local (this machine; Docker daemon absent, so live-container legs
environmentally skipped — mock suites cover argv/isolation logic):

- `cargo test -p remote_runner` → 17 passed, 0 failed (exit 0)
- `cargo test -p tool_runtime remote_` → 5 passed, 0 failed (exit 0)
- `cargo fmt --check` (crates + desktop) → clean; `cargo clippy
--workspace --all-targets -- -D warnings` → clean (exit 0)
- `cargo test --workspace` → all suites green (exit 0; incl. 93
  tool_runtime, 66 evidence_store with 1 pre-existing ignore)
- `cargo check --all-targets` (desktop shell, incl. B3 fix) → clean
- `pnpm install --frozen-lockfile` (CI=true) → lockfile up to date, exit 0
- contracts + browser-use builds → exit 0
- `pnpm lint` → green after removing machine-local `apps/ui/.next/` +
  `apps/ui/out/` build output (untracked, pre-existing; environment
  finding, not a repo change)
- `pnpm typecheck` → exit 0; `pnpm test` → exit 0 (root 35, contracts
  110, audit 29, browser-use 57, ui 186); `pnpm build` → exit 0
- `pnpm audit --audit-level high` (CI gate) → exit 0 (one _moderate_
  minimatch below gate — informational, pre-existing on main, recorded)
- `pnpm audit:swarm` → WARN (0 criticals), exit 0; generated
  `docs/reports/audit-latest.*` reverted (run artifacts, not committed)

CI on PR (post-rebase + fix): run `43275815075` — `success`. Checks:
`setup/meta`, `typescript`, `rust-core`, `desktop-check`, `audit`,
`media-check`, `awakening`, `hardening`, `stress-shutdown`
(ubuntu+windows) all pass; `browser live smoke` skipping (manual-only,
legitimate per `docs/CI.md`); `flake-watch` pass.

## 5. Follow-up WPs (scoped, not built)

- WP-F1 (B4 hardening): resolve SSH credentials via OS keychain /
  ssh-agent instead of key files; enforce 0600 on referenced keys at
  `store_runner` time. Small; needs owner decision on UX.
- WP-F2: dedicated test asserting serialized `RunnerStatusView` +
  `ToolReceipt` JSON contains no key material or key paths (currently
  true by construction + shape review; make it a regression test).
- WP-F3: Docker-live + SSH-against-local-container legs in CI
  (currently mock-only; needs a container runtime on runners).
- WP-F4: per-runner spend caps + `usage_records` wiring for remote exec
  (P9 estimate accuracy for BYOC compute).
- WP-F5: `set_task_runner` assignment audit event (who routed which task
  where, when) in the evidence ledger.

## 6. Merge SHA + CI URLs + status

| Item                 | Value                                                                                                                    |
| -------------------- | ------------------------------------------------------------------------------------------------------------------------ |
| Merge                | `e286df2` Merge pull request #27 (`--merge`, branch auto-deleted)                                                        |
| In-scope fix         | `b0e99aa` (included in merge)                                                                                            |
| Rebased base         | `origin/main` `8a17456`                                                                                                  |
| CI (PR, post-rebase) | `https://github.com/shubhanand021-EXA/talos/actions/runs/43275815075` — `success`                                        |
| CI (main, merge)     | `https://github.com/shubhanand021-EXA/talos/actions/runs/43276343979` — `success`; CodeQL main `43276343301` — `success` |
| Docs commit          | `83ad141` record BYOC merge (PR #32 squash, CI `43278502686` — `success`)                                                |
| Status               | GREEN on PR; main verdict pending watch                                                                                  |
