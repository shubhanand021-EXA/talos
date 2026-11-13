# Report 006 — Verify-and-merge: gateway daemon auth hardening (PR #24)

- Date: 2026-11-13 (UTC)
- Agent: OpenCode (sole engineer)
- Batch: 1 / Prompt: 006 (review-and-merge; security fix built in-scope)
- Read order followed: `AGENTS.md` → `VISION.md` → `reports/001-*`, `002-*`,
  `003-*`, `004-*`, `005-*` → `docs/CI.md` (required set read, never hardcoded)
- Branch strategy: `gh pr checkout 24` → rebase onto post-merge `main`
  (`2bc4a4f`) → one in-scope hardening commit (`5f0e3e7`) → push → CI green
  → `--merge` (history preserved per prompt; branch auto-deleted)

## 1. Rebase log

- PR #24 (`feat/gateway-daemon`, 1 commit on old base `04c80aa`) checked out clean.
- `git fetch origin` → `git rebase origin/main`: the single upstream commit
  `4076293` was **skipped as already applied** — the daemon code had already
  landed on `main` via PR #27 (as `8e6df19` + follow-ups), so the rebased
  branch was byte-identical to `main` (empty `git diff origin/main...HEAD`).
- License conflicts: none arose; post-rebase grep confirms zero `MIT` in
  `*.toml`/`package.json` and `LICENSE` still full Apache-2.0.
- No code was rebuilt: the review below targets the daemon as merged,
  and the hardening commit adds only the missing auth layer.

## 2. Daemon architecture

`crates/talos_gateway` (binary `talos-gateway` + lib): owns the exclusive
evidence WAL lock, serves loopback WebSocket clients on `127.0.0.1:8080/talos`.
Transports: argv-exact local process spawns only (no inbound ports beyond the
one loopback socket; no tunnel/port-forward/webhook code anywhere — verified
by grep for `0.0.0.0|listen|TcpListener|port-forward|webhook|ngrok`).

- Lifecycle: `main.rs` boots `GatewayState` (recovery replay), binds, persists
  token+PID files, parks on Ctrl-C, then bounded-drain shutdown (`5s` grace,
  abort remainder). Survives desktop close by design (detached child).
- IPC surface: `SubmitTask` (durable queue + policy broker engine),
  `CancelTask`, `ApprovePolicy` (ledger-record only — real grants live in the
  desktop `ApprovalStore`), `Ping`/`GetStatus`, `ForwardIpc` over a fixed
  allowlist (status/tasks/verify/schedules/channels/profiles/VM/HITL routes;
  unknown commands deny with `unknown_ipc`).
- Credential flow: per-start 256-bit token (OS CSPRNG, `uuid v4`) minted in
  `build_full`, written owner-only **after** a successful bind (a failed
  second instance can never clobber the running daemon), read by the Tauri
  shell from the same app-data dir; presented as a
  `talos-gw-token.*` subprotocol element + `Origin: tauri://localhost`.
  Never in URLs, never logged (verified by grep + `auth.rs` tests).
- Restart: `restart_daemon` best-effort kills the PID-file occupant, then
  spawns fresh; the token file is rewritten by the winner only.

## 3. Security review table B1–B7

| #   | Item                                                               | Verdict           | Evidence (file:line)                                                                                                                                                                                                                                                                                                                                                                                          |
| --- | ------------------------------------------------------------------ | ----------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| B1  | Binds 127.0.0.1 ONLY; non-loopback refused                         | PASS              | `TcpListener::bind(LOCALHOST)` (`server.rs:59`); peer check `peer_allowed` (`server.rs:135+`); negative tests `gateway_binds_loopback_only`, `loopback_peers_...` (`server.rs` tests)                                                                                                                                                                                                                         |
| B2  | Origin validation on every upgrade, before token exchange          | PASS (fixed here) | **Was absent.** Added `accept_hdr_async` callback: route → Origin → token order (`server.rs`); exact-match allowlist (`protocol.rs` `ALLOWED_ORIGINS`); negative live-socket tests: cross-origin-with-valid-token rejected, missing Origin rejected (`server.rs` tests)                                                                                                                                       |
| B3  | Per-start token, owner-only/keychain, never logged/URL             | PASS (fixed here) | **Was absent.** Added: `generate_auth_token` (`protocol.rs`), `auth.rs` file IO (0600 on Unix), write-after-bind (`main.rs`); negative live-socket tests: missing/stale token → 401, query-only token rejected (`server.rs` tests); token round-trip + mode tests (`auth.rs` tests)                                                                                                                           |
| B4  | No client-supplied gateway URL trusted                             | PASS              | `GATEWAY_URL` is a hardcoded const on both sides (`contracts/gateway.ts:13`, `desktop gateway.rs:16`); no `gatewayUrl`/`location.search`/`URLSearchParams` anywhere in UI (grep); regression tests: URL has no `?`, client never reads page URL (`gateway.test.ts` `never consults the page URL`)                                                                                                             |
| B5  | No inbound tunnel/forward/webhook exposure                         | PASS              | Grep finds only the loopback listener; channels are outbound long-poll adapters with keychain tokens (`channels/registry.rs:257-280`); VNC loopback-only (`vm.rs:496`, test asserts)                                                                                                                                                                                                                          |
| B6  | Bounded drain shutdown; survives UI close; restart/crash interplay | PASS (fixed here) | **Was fire-and-forget.** Added `ServerHandle::shutdown` (JoinSet + 5s grace + abort remainder), `Drop` still signals; `shutdown_stops_accepting` test; restart kills previous occupant via PID file (`gateway.rs` desktop); crash recovery via `run_startup_recovery` at boot (pre-existing, tested). Residual: WAL checkpoint + envelope re-seal on shutdown remains future work (`main.rs` comment) → WP-F3 |
| B7  | Pairing/approval paths: caller scope, no self-escalation           | PASS              | `ApprovePolicy` records a ledger event only (`server.rs:263-305`, `via: talos-gateway/1`) — the grant itself lives in the desktop `ApprovalStore`; link-channel requires the token and stores it straight to the keychain, never the ledger (`state.rs:771-785`); policy broker remains the sole authority for side effects (`state.rs:submit_task` via `self.engine`)                                        |

Zero unresolved P0 gaps. No BLOCKED report needed (all failures fixed in-scope on attempt 1).

## 4. Negative-test inventory + results

New tests (all passing, local + CI):

- `protocol::origin_allowlist_is_exact` — 4 allowlisted origins pass; 9 hostile
  variants (suffix tricks, wrong ports, empty) rejected.
- `protocol::token_extracted_from_subprotocol_offer_only` — extraction from
  subprotocol header only; `?token=` query strings yield nothing.
- `protocol::generated_tokens_are_unique_and_tchar_safe` — uniqueness + header-safe.
- `server::gateway_binds_loopback_only`, `loopback_peers_...` (B1).
- `server::handshake_validation_order_and_codes` — 404/403/401 order + happy path.
- `server::live_upgrade_rejects_cross_origin_before_token` (B2, real socket).
- `server::live_upgrade_rejects_missing_and_stale_tokens` (B3, real socket).
- `server::live_upgrade_never_accepts_query_tokens` (B3+CVE-25253, real socket).
- `server::live_upgrade_accepts_valid_origin_and_token` (positive control ×2 origins).
- `server::shutdown_stops_accepting` (B6).
- `auth::token_round_trips_and_never_empty`, `pid_round_trips`,
  `token_file_is_owner_only` (Unix 0600).
- `gateway_e2e.rs` updated: all 5 E2E tests authenticate (were 403-failing).
- Desktop `gateway.rs`: `upgrade_request_carries_origin_and_no_query_token`,
  `stop_previous_daemon_ignores_missing_and_bad_pids` (compile-verified;
  desktop test binary cannot execute on this Windows box — pre-existing
  environment finding, same as CI which runs fmt/clippy/check only).
- TS: `gateway.test.ts` 3 handshake-auth tests; `contracts/gateway.test.ts`
  origin/token-shape pin test.

Local results: `cargo test -p talos_gateway` 21 lib + 14 integration, all pass;
`cargo test --workspace` exit 0; desktop `cargo clippy --all-targets` clean;
`pnpm` lint/typecheck/test (contracts 111, ui 189)/build/`audit:swarm` green.

## 5. Follow-up WPs (scoped, not built)

- WP-F1: keychain-backed daemon token (in addition to the 0600 file) for
  managed/enterprise profiles; token rotation without restart.
- WP-F2: WAL checkpoint + envelope re-seal on graceful shutdown (today:
  crash-recovery replay on next boot covers it).
- WP-F3: service-install units (launchd/systemd/Windows service) — explicitly
  deferred by the prompt; today the desktop shell spawns the daemon.
- WP-F4: per-client Origin allowlist override for advanced self-hosted
  topologies (today: fixed allowlist; any change needs an ADR).
- WP-F5: rate-limit repeated failed handshakes per peer (defense in depth;
  loopback-only bounds the exposure today).

## 6. Merge SHA + CI URLs + status

| Item              | Value                                                                                                                                                                                                                        |
| ----------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Merge             | `89cad29` Merge pull request #24 (`--merge`, branch auto-deleted)                                                                                                                                                            |
| In-scope fix      | `5f0e3e7` (included in merge)                                                                                                                                                                                                |
| Rebased base      | `origin/main` `2bc4a4f`                                                                                                                                                                                                      |
| CI (PR, post-fix) | run `43402816445` — `success` (all 6 ruleset + awakening/hardening/stress-shutdown ubuntu+windows; browser smoke manual-skip legitimate)                                                                                     |
| CI (main, merge)  | `https://github.com/shubhanand021-EXA/talos/actions/runs/43403254030` — `success`; CodeQL main `43403254646` — `success`                                                                                                     |
| Docs commit       | PR #36 squash (`docs(release): record gateway auth hardening ... [Prompt-006]`); this report rides in that commit                                                                                                            |
| B4 residual note  | CVE-2026-25253 class structurally excluded: no `gatewayUrl` trust, Origin allowlisted, token never in URL; CVE-2026-33579 class excluded: no self-approval path (approvals are ledger records, grants live in desktop store) |
