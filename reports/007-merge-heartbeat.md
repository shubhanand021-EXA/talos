# Report 007 — Verify-and-merge: HEARTBEAT scheduler (PR #25)

- Date: 2026-11-13 (UTC)
- Agent: OpenCode (sole engineer)
- Batch: 1 / Prompt: 007 (review-and-merge; in-scope hardening fixed, never rebuilt)
- Read order followed: `AGENTS.md` → `VISION.md` → `reports/001-*` … `006-*`
  → `docs/CI.md` (required set read, never hardcoded)
- Branch strategy: `gh pr checkout 25` → rebase onto post-merge `main`
  (`701e274`) → one in-scope hardening commit (`5d627ed`) → push → CI green
  → `--merge` (history preserved per prompt; branch auto-deleted)

## 1. Rebase log

- PR #25 (`feat/scheduler-heartbeat`, 2 commits on old base `04c80aa`)
  checked out; `git fetch origin` → `git rebase origin/main` replayed both
  as already-applied (skipped `4076293` + `08187ad` — the scheduler code
  had landed on `main` via PR #27). Rebased branch was byte-identical to
  `main` (empty diff) before the hardening commit.
- License conflicts: none arose; post-rebase grep confirms zero `MIT` in
  `*.toml`/`package.json`; `pnpm-lock.yaml` byte-identical (no dependency
  changes anywhere in this batch).
- In-scope hardening commit (`5d627ed`, +171/−6 across 5 files):
  unattended default budget cap, deterministic idempotency tests, bounded
  drain of background loops.

## 2. Pipeline summary

Parse → schedule → dispatch, all deterministic and time-explicit:

- **Parse** (`instruction_parser/src/heartbeat.rs`): bounded input (256 KiB
  file, ≤128-char names, ≤32 tasks, fail-closed `Err` on garbage); sections
  map to `ScheduledTask { kind, every_minutes | at | weekday | day_of_month,
payload }`; no shell anywhere — payload is an opaque goal string.
- **Schedule** (`crates/scheduler`): `Trigger::next_after(now, last_run)`
  total over UTC; `Scheduler::tick_once(now)` returns due schedules and
  advances their triggers _before_ dispatch (at-most-once per interval);
  missed intervals are skipped, never caught up (bounded).
- **Dispatch** (`talos_gateway::state::run_scheduler_tick`, every 30s):
  `submit_system_task` → `submit_external_task` → Supervisor keyword DAG →
  policy gate (`build_gate_request` via `channels::classify_goal`) → Allow =
  execute through the broker / RequireHumanReview = park in
  `waiting_policy` + desktop notification / Deny = terminal `failed`.
  Every branch ledgered (`scheduler.fired`, `.policy_required`, `.denied`
  events + TaskStarted/Completed broadcasts).
- HEARTBEAT.md is read from `<app_data_dir>/HEARTBEAT.md` (production) or
  absent (empty scheduler); the gateway 30s tick is the only driver —
  no filesystem watcher, no hot reload (WP-F3).

## 3. Autonomy review table B1–B8

| #   | Item                                                     | Verdict            | Evidence (file:line)                                                                                                                                                                                                                                                                                                                                                     |
| --- | -------------------------------------------------------- | ------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| B1  | Every job via the same default-deny broker               | PASS               | `submit_external_task` evaluates `DefaultPolicyEngine` before any execution (`state.rs:submit_external_task`); `execute_task` runs through the broker; high-risk/destructive tests below prove no bypass                                                                                                                                                                 |
| B2  | Unattended: high-risk parks or denies, never auto-runs   | PASS               | `classify_goal` tiers (wildcard-destructive → default-deny shape; high-risk → HumanReview) (`channels/classifier.rs:17-36,75-113`); pre-existing `risky_goal_parks_in_waiting_policy_with_notification`; **new** `destructive_goal_fails_closed_without_executing` (`gateway_scheduler.rs` tests: `rm -rf /tmp/cache` → terminal `failed`, zero TaskStarted)             |
| B3  | Per-job budget cap; over-budget ⇒ deny before traffic    | PASS (fixed here)  | **Gap found: scheduled tasks set no cap (`None` = uncapped).** Fix: `DEFAULT_EXTERNAL_TASK_BUDGET_USD = 1.0` recorded on every external task row at submit (`state.rs`), fail-closed (`?`); governor deny-before-traffic already tested (`llm_gateway ...::denies_over_budget_before_any_call`); **new** boundary test `scheduled_tasks_carry_the_unattended_budget_cap` |
| B4  | Idempotency: a firing twice executes once                | PASS (tests added) | Mechanism: advance-before-dispatch in `tick_once` + forward-anchored rebuild after restart. **New** deterministic tests: `tick_fires_due_and_advances_without_double_fire` (pre-existing), `rebuild_after_fire_does_not_refire_the_same_interval`, `clock_skew_backward_never_refires`                                                                                   |
| B5  | Missed-wake recovery bounded, no thundering herd         | PASS (test added)  | One fire per schedule per tick regardless of gap size (no per-missed-interval catch-up). **New** `far_future_jump_fires_each_schedule_exactly_once` (+1 year jump → exactly 2 fires, then silence)                                                                                                                                                                       |
| B6  | Every run ledgered; verification passes after a run      | PASS               | `scheduler_trigger_submits_system_task` asserts task row + TaskStarted/Completed + `run_count`; fired/policy/denied events on every path (`submit_external_task`); all new tests assert ledger rows/status                                                                                                                                                               |
| B7  | NL parsing injection-safe; maps only to registered tools | PASS               | Bounded parser (256 KiB / 128-char / ≤32 tasks, `heartbeat.rs:63-127`); goal is data to the keyword planner (unknown tools rejected — pre-existing planner tests); executor allowlists + policy gates downstream; slug sanitized `[a-z0-9-]` (`engine.rs:87-103`); dangerous goals demonstrably denied (B2 test)                                                         |
| B8  | Shutdown drains via bounded contract                     | PASS (fixed here)  | **Gap found: heartbeat/scheduler loops were detached `tokio::spawn`, surviving `shutdown()`.** Fix: loops moved into a dedicated `JoinSet` aborted immediately on shutdown; connections keep the 5s bounded drain (`server.rs`); **existing** `shutdown_stops_accepting` still green                                                                                     |

Zero unresolved P0 gaps. No BLOCKED report needed (all gaps fixed in-scope on attempt 1; nothing was loosened to make a test pass).

## 4. Deterministic-test inventory + results

New tests (all deterministic — explicit `now`, zero real waiting):

- Scheduler engine (`crates/scheduler/src/engine.rs`): `far_future_jump_...`,
  `rebuild_after_fire_...`, `clock_skew_backward_...` (3 new; 16/16 pass).
- Gateway integration (`gateway_scheduler.rs`): `destructive_goal_...`,
  `scheduled_tasks_carry_the_unattended_budget_cap` (2 new; 6/6 pass).
- Pre-existing suites all green: scheduler, talos_gateway (35), policy_broker,
  agent_runtime, llm_gateway, evidence_store; full `cargo test --workspace`
  exit 0; desktop `cargo check` clean; pnpm lint/typecheck/test
  (contracts 111, ui 189)/build/`audit --audit-level high` exit 0;
  `audit:swarm` WARN-only, 0 criticals (privacy WARNs are pre-existing
  origin-allowlist false positives, triaged — same as Prompt 006).
- CI (PR, post-fix): run `43407702728` — `success` (all 6 ruleset +
  awakening/hardening/stress-shutdown ubuntu+windows; browser smoke
  manual-skip legitimate).

## 5. Follow-up WPs (scoped, not built)

- WP-F1: per-schedule budget overrides (UI/API to raise/lower the $1.00
  unattended default; owner-only change).
- WP-F2: WAL checkpoint + envelope re-seal on graceful shutdown (today: crash
  replay on next boot covers durability; Prompt 006 carry-over).
- WP-F3: HEARTBEAT.md hot-reload/file-watch (today: read once at daemon boot;
  restart picks up edits; document in onboarding).
- WP-F4: missed-fire observability (ledger a `scheduler.skipped` event with
  the skipped interval count for operator visibility).
- WP-F5: schedule-level kill-switch independent of the global e-stop
  (today: pause flag + global e-stop cover it).

## 6. Merge SHA + CI URLs + status

| Item              | Value                                                                                                                    |
| ----------------- | ------------------------------------------------------------------------------------------------------------------------ |
| Merge             | `c1f003e` Merge pull request #25 (`--merge`, branch auto-deleted)                                                        |
| In-scope fix      | `5d627ed` (included in merge)                                                                                            |
| Rebased base      | `origin/main` `701e274`                                                                                                  |
| CI (PR, post-fix) | run `43407702728` — `success` (full required + convention set)                                                           |
| CI (main, merge)  | `https://github.com/shubhanand021-EXA/talos/actions/runs/43408081625` — `success`; CodeQL main `43408082060` — `success` |
| Docs commit       | PR #37 squash (`docs(release): record HEARTBEAT scheduler merge ... [Prompt-007]`); this report rides in that commit     |
| Flake policy      | No reruns needed this prompt (zero red gates)                                                                            |
