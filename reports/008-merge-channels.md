# Report 008 — Verify-and-merge: Telegram + Discord channels (PR #26)

- Date: 2026-11-13 (UTC)
- Agent: OpenCode (sole engineer)
- Batch: 1 / Prompt: 008 (review-and-merge; in-scope hardening fixed, never rebuilt)
- Read order followed: `AGENTS.md` → `VISION.md` → `reports/001-*` …
  `007-*` → `docs/CI.md` (required set read, never hardcoded)
- Branch strategy: `gh pr checkout 26` → rebase onto post-merge `main`
  (`4912514`) → one in-scope hardening commit (`8bce957`) → push → CI green
  → `--merge` (history preserved per prompt; branch auto-deleted)

## 1. Rebase log

- PR #26 (`feat/messaging-channels`, 3 commits on old base `04c80aa`)
  checked out; `git fetch origin` → `git rebase origin/main` replayed all
  three as already applied (skipped `4076293`, `08187ad`, `ee23843` — the
  channel code had landed on `main` via PR #27). Rebased branch was
  byte-identical to `main` (empty diff) before the hardening commit.
- License conflicts: none arose; post-rebase grep confirms zero `MIT` in
  `*.toml`/`package.json`; `pnpm-lock.yaml` byte-identical (no dependency
  changes anywhere in this batch).
- In-scope hardening commit (`8bce957`, +292/−32 across 4 files):
  bot-author filter, token scrubbing, typed 429s, send pacing.
- Adjacent prompts' merged work re-verified as part of this review (not
  rebuilt): the unattended $1/task budget cap, JoinSet drain, scheduler
  idempotency tests (Prompt 007, on `main`), gateway handshake auth
  (Prompt 006, on `main`).

## 2. Adapter architecture

`crates/channels`: transport-only adapters behind `ChannelAdapter`
(outbound `post_json`/`get` via `HttpClient`; parsing inbound; never
authorize, never execute). Telegram = Bot API long-poll `getUpdates` +
`sendMessage` (token in path per Bot API design); Discord = webhook POST
for sends + bot-REST `GET /channels/{id}/messages` poll (bot token only in
the `Authorization` header). `ChannelRegistry` owns adapters, seen-id
dedup, bounded 50-row metadata-only log, keychain-backed linking
(`link_channel` stores to vault + flips enable + persists secret-free
config; `unlink_channel` removes + disables + detaches).

Message flow: 30s daemon tick → `poll_channels` → per-adapter
`receive_updates` → gateway `handle_channel_message` (allowlist check →
ledger `channel.received` → Supervisor DAG → policy broker gate →
`waiting_policy` park + desktop notification, or `denied` terminal, or
execute-through-broker) → `send_reply` templated receipt (bounded,
never raw tool output) → UI log entry. Failures never fail the tick;
every path is ledgered (`channel.received`, `channel.rejected`,
`scheduler/task` fired/policy/denied events, `TaskStarted/Completed`).

Token handling: user pastes once into a password-type UI field (cleared on
submit, never displayed back) → `link_channel` → OS keychain only; config
file stays secret-free (tested); adapters hold the token in memory only;
status views and logs never carry tokens.

## 3. Channel review table B1–B7

| #   | Item                                                                              | Verdict           | Evidence (file:line)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| --- | --------------------------------------------------------------------------------- | ----------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| B1  | Outbound-only to official bot APIs; no inbound listener/tunnel/webhook exposure   | PASS              | Only `post_json`/`get` call sites (`telegram.rs`, `discord.rs`); `receive_webhook` is a pure parser (no socket); grep for `TcpListener                                                                                                                                                                                                                                                                                                                                                                                       | listen      | bind     | 0.0.0.0  | tunnel   | ngrok   | HttpServer | axum  | actix                                                                                                                                                       | warp`over`crates/channels`→ zero hits; bot APIs are`api.telegram.org`+`discord.com/api/v10` |
| B2  | Unpaired-sender ⇒ ignored + ledgered, never executed                              | PASS              | `handle_channel_message` allowlist gate (`state.rs:641-684`): chat/channel-id allowlist, empty ⇒ deny-all; rejection ledgered (`channel.rejected`); pre-existing `non_allowlisted_sender_is_dropped_silently` test; no per-sender pairing (chat-level pairing by design — WP-F1)                                                                                                                                                                                                                                             |
| B3  | Outbound sends policy-gated; high-risk never auto-sends; unapproved send blocked  | PASS (test added) | Sends are canned `render_reply` templates (bounded, no raw tool output); the underlying task passes the full broker gate first. **New** negative test: `send_to_unknown_channel_fails_closed` (unlinked channel ⇒ Err, nothing ledgered as sent); pre-existing `remote_destructive_command_is_broker_denied` covers the refused path                                                                                                                                                                                         |
| B4  | Tokens: OS keychain only; never in config/logs/evidence/errors; redaction by test | PASS (fixed here) | **Gap found: reqwest embeds the request URL (carrying the Telegram token / Discord webhook URL) in transport errors, which the poll/reply paths log.** Fix: `scrub()` in both adapters applied to every Transport error + no-token-in-Debug (manual `Debug` impls omit secrets; `status()` carries booleans only). **New** tests: `transport_errors_never_carry_the_token` / `..._secrets` (token-bearing error in ⇒ scrubbed error out); pre-existing `save_round_trip_is_secret_free`, `error_messages_never_carry_values` |
| B5  | Per-platform rate limiting; backoff on 429s; no retry storms                      | PASS (fixed here) | **Was: no explicit 429 path (lumped into Rejected).** Fix: typed `ChannelError::RateLimited` (no body, no retry — the 30s tick is the only retry, so storms are structurally impossible) + 1s per-adapter send pacing via pure `throttle_delay` (unit-tested). **New** tests: 429 → `RateLimited` for both adapters; `throttle_delay_paces_bursts`                                                                                                                                                                           |
| B6  | Every cycle ledgered; fidelity intact (mock default, live opt-in)                 | PASS              | `channel.received` / `channel.rejected` / fired / policy / denied events + TaskStarted/Completed broadcasts (`state.rs`); MockChannelAdapter for hermetic tests; live adapters only constructed with keychain credentials (`from_config` skips unlinked)                                                                                                                                                                                                                                                                     |
| B7  | No unofficial protocols anywhere in the diff                                      | PASS              | Grep for `whatsapp                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           | signal\.org | imessage | pyrogram | telethon | baileys | userbot    | tdlib | session \+ api_id`→ zero product hits (only false positives:`sessionId` locals, crate names, stdlib mentions); Telegram Bot API + Discord REST/webhook only |

Two real gaps found and fixed in-scope (token-in-error-logs, bot self-trigger loop — see below); zero P0 gaps remain. No BLOCKED report needed.

Additional hardening in this batch (beyond the B-items, same files): **bot-authored messages are dropped at parse time in both adapters** (Telegram `is_bot`, Discord `author.bot`). Without this, the daemon's own Discord replies (visible in the polled channel) would re-enter as new tasks — an unbounded self-trigger loop — and any third-party bot in an allowed chat could inject tasks. Covered by new `bot_authored_..._never_become_tasks` tests for both adapters.

## 4. Test inventory + results (live smoke status)

New tests (all passing, local + CI): `token_extracted...` n/a (006) — this batch:
`transport_errors_never_carry_the_token`, `transport_errors_never_carry_secrets`,
`bot_authored_updates_never_become_tasks`, `bot_authored_messages_never_become_tasks`,
2× `rate_limit_maps_to_typed_error_without_body`, `throttle_delay_paces_bursts`,
`send_to_unknown_channel_fails_closed` (8 new). Full `cargo test --workspace`
green (channels 31/31); desktop `cargo check` clean; pnpm lint/typecheck/test
(contracts 111, ui 189)/build/`audit:swarm` (WARN-only) green.
CI (PR, post-fix): run `43413592475` — `success` (all 6 ruleset +
awakening/hardening/stress-shutdown ubuntu+windows; manual smoke skipped).
**Live smoke: DEFERRED — no test bot tokens available** (acceptable per
prompt; mock transports cover argv/isolation/parsing; CI runs ubuntu+windows
mock suites). No real external hosts touched; no credentials committed.

## 5. Batch-5 completion table

| PR  | Scope                                                       | Merge SHA | CI (PR)                                             | Status |
| --- | ----------------------------------------------------------- | --------- | --------------------------------------------------- | ------ |
| #24 | Gateway daemon + WS IPC, auth-hardened (Prompt 006)         | `89cad29` | `43402816445` success                               | MERGED |
| #25 | HEARTBEAT scheduler, unattended-hardened (Prompt 007)       | `c1f003e` | see `reports/007-merge-heartbeat.md` §6 — `success` | MERGED |
| #26 | Telegram + Discord adapters, channel-hardened (this prompt) | `d233398` | `43413592475` success                               | MERGED |
| #27 | BYOC SSH/Docker + wedge integration (Prompt 005)            | `e286df2` | `43275815075` success                               | MERGED |

Batch 5 (gateway, scheduler, channels, BYOC) is fully merged: every line
of the Batch-5 track now lives on `main` behind the same policy broker,
evidence ledger, and audit gates. Remaining Batch-5+ roadmap items (Slack,
voice/media, teams, advanced browser auth, marketplace, mobile, IDE
extensions) stay explicitly unchecked in README/ROADMAP.

## 6. Follow-up WPs (scoped, not built)

- WP-F1: per-sender pairing for Telegram/Discord (today: chat/channel
  allowlist; 1:1 DMs are effectively paired, group chats are chat-scoped).
- WP-F2: durable poll cursors + seen-id set across restarts (today: bounded
  one-window replay, deduped only within a process lifetime; replays are
  policy-gated + budget-capped so the blast radius is small).
- WP-F3: Retry-After-aware backoff using response headers (today: typed
  429, no retry, 30s tick; needs `HttpClient` header surfacing).
- WP-F4: `link_channel` over the authenticated gateway socket is trusted UI
  input by design; consider an explicit human-confirmation IPC for linking
  (today: add/remove are single-user local actions).
- WP-F5: live-smoke harness gated on env-provided test bot tokens.

## 7. Merge SHA + CI URLs + status

| Item              | Value                                                                                                                                                                                                                                                                                                                                  |
| ----------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Merge             | `d233398` Merge pull request #26 (`--merge`, branch auto-deleted)                                                                                                                                                                                                                                                                      |
| In-scope fix      | `8bce957` (included in merge)                                                                                                                                                                                                                                                                                                          |
| Rebased base      | `origin/main` `4912514`                                                                                                                                                                                                                                                                                                                |
| CI (PR, post-fix) | run `43413592475` — `success` (full required + convention set)                                                                                                                                                                                                                                                                         |
| CI (main, merge)  | run `43413993015` — `failure` on attempt 1 (`db::tests::plaintext_database_migrates_in_place`, known drain_pool flake class), then `success` on a single failed-job rerun (attempt 2, policy-compliant); tracked in [#38](https://github.com/shubhanand021-EXA/talos/issues/38) + FLAKES.md row. CodeQL main `43413993655` — `success` |
| Docs commit       | PR #39 squash (`docs(release): record channels merge ... [Prompt-008]`); this report rides in that commit                                                                                                                                                                                                                              |
| Flake policy      | ONE rerun used on main run 43413993015 (attempt 1 red → attempt 2 green); tracked issue #38; FLAKES.md recurrence row added (same commit batch)                                                                                                                                                                                        |
| Live smoke        | Deferred — no test bot tokens (acceptable per prompt)                                                                                                                                                                                                                                                                                  |
