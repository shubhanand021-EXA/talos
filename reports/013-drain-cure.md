# Report 013 — sqlx 0.9 cures upstream pool-close race #3217 (Prompt 013)

- Date: 2026-11-14 (UTC)
- Agent: OpenCode (sole engineer)
- Batch: 2 / Prompt: 013 (drain cure; architect-authorized sqlx 0.8.6 → 0.9.x ONLY)
- Read order followed: `AGENTS.md` → `VISION.md` → `reports/002-drain-pool-fix.md` →
  `reports/010-batch-1-audit-BLOCKED.md` → `docs/EVIDENCE.md` → `docs/evidence/FLAKES.md` →
  `docs/CI.md` (+ `.talos/baselines.json` for the P3 context)
- Branch: `fix/sqlx09-drain-cure-prompt-013` (from `main` `df5df4c`)

## 1. Upstream evidence summary (#3217 / #3952)

- sqlx issue #3217: `PoolInner::close` (0.8.x) has two bugs — idle conns not
  always drained before the loop ends, and a semaphore permit fabricated out
  of thin air when idle conns are treated as live — causing `close()` to
  return prematurely. Fixed by PR #3952, released in sqlx **0.9 ONLY** (latest
  stable 0.9.x = **0.9.0**, published 2026-05-21, `rust_version` 1.99.1-compatible 1.94.0);
  NOT backported to 0.8.6.
- Source-verified both sides in the local cargo registry cache:
  - 0.8.6 (`sqlx-core-0.8.6/src/pool/inner.rs:97-115`): close loop drains
    idle via `idle.live.float(...).close().await` — `float()` mints a fresh
    `DecrementSizeGuard` whose drop does `size -= 1` **and**
    `semaphore.release(1)`, fabricating one permit per drained idle conn;
    the `acquire(1..=max)` ladder then succeeds without waiting.
  - 0.9.0 (`sqlx-core-0.9.0/src/pool/inner.rs:96-119`): permits are acquired
    FIRST (`acquire(max_connections)` — close genuinely waits), idle is
    drained fully via raw `close()` (no guard, no fabricated permit), then
    `num_idle.store(0)` + `size.store(0)`. Stale-counter diagnostics cured too.
- Our 002 bounded contract (`drain_pool`: pre-close idle capture, 30s close
  bound, 30s drain poll) stays intact as defense-in-depth — it made the
  failure loud/bounded but could not cure the upstream race. 010 correctly
  blocked the gate on this.

## 2. Reproducer design + FAILING 0.8.6 output

`db::tests::pool_close_waits_for_checked_out_connection`
(`crates/evidence_store/src/db.rs`): max_connections=2, hold A checked out,
return B to idle (precondition asserted), then `close()` under a 500ms
timeout. Deterministic by construction — the fabricated permits satisfy the
2-rung ladder with zero timing dependence; on fixed sqlx the same code
pends (then completes after `drop(live)`, asserting `size() == 0`).

0.8.6 output (5/5 FAIL, each ~0.00–0.01s — no timing dependence):

```text
test db::tests::pool_close_waits_for_checked_out_connection ... FAILED
thread '...' panicked at evidence_store\src\db.rs:694:9:
pool.close() returned while a connection was still checked out (upstream sqlx#3217)
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 66 filtered out; finished in 0.01s
```

## 3. Upgrade diff (call sites, features, lockfile)

- `crates/Cargo.toml`: `sqlx 0.8 → 0.9`, features `sqlite → sqlite-bundled`
  (verified against 0.9.0 metadata: `sqlite-bundled` == 0.8.6's `sqlite`
  exactly; top-level `sqlite` now also pulls deserialize/load-extension/
  unlock-notify — declined). `runtime-tokio`/`macros`/`migrate` unchanged.
- Lockfiles: `crates/Cargo.lock` + `apps/desktop/src-tauri/Cargo.lock` →
  sqlx* exclusively 0.9.0; `libsqlite3-sys 0.30.1` inside the required
  window (>=0.30.1,<0.38.0). `pnpm-lock.yaml` byte-identical.
- Dynamic-SQL call sites (0.9 accepts only `&'static str`/`AssertSqlSafe`):
  1. `ledger.rs:338` — allowlist `match` returning `&'static str` (dynamic
     SQL eliminated, `InvalidId` preserved).
  2. `storage.rs:239,371,377,381` — `AssertSqlSafe(format!(...))` (VACUUM
     INTO / ATTACH / DELETE+INSERT take no bind params; quoting by
     `sqlite_quote`/`ident_quote` audited and kept as the defense).
  3. `vector_store/lib.rs:274` — `AssertSqlSafe(sql)` (filter-built
     structure; every value still bound).
  4. `cost_tracker/metrics.rs:383` — params narrowed to `&'static str`
     (call sites already pass literals).
  5. `tool_rag/index.rs:58` — no change (array of `&'static str` literals).
  6. `apps/desktop/src-tauri/Cargo.toml` — removed dead `sqlx 0.8` dep (zero
     uses in `src/`); this evicts 0.8.6 from the desktop binary too.
- `drain_pool` + 30s bounds + `stress-shutdown` detector: byte-identical,
  untouched. 0.9's dropped-write rollback changes nothing we depend on.

## 4. PASSING 0.9.x output + stress results

- Reproducer on 0.9.0: 5/5 PASS (~0.5s each — the 500ms timeout elapses as
  designed, then clean close, `size() == 0`).
- 2-CPU affinity loop (no Docker/WSL on this box; `ProcessorAffinity=3`):
  20/20 PASS from per-iteration logs (the `$p.ExitCode` readings were a
  launcher artifact — logs authoritative: `test result: ok. 1 passed`).
  Pre-upgrade rate on identical hardware: deterministic 5/5 FAIL.
- 100-cycle seal stress (`--ignored pool_shutdown`): 100/100 PASS (56.48s).
- Full regression: `cargo fmt --check` 0, `clippy --workspace
--all-targets -D warnings` 0, `cargo test --workspace` 0 (68 suites);
  desktop `fmt`/`clippy`/`check --all-targets` 0; `pnpm lint/typecheck/test/
build` 0 (ui 40 files/194 tests); `pnpm audit` 0 (1 moderate
  pre-existing); `audit:swarm` WARN-only, 0 criticals; parity validator green.
- Full-workspace test run had one transient: `app.test.tsx` route test timed
  out (5s) mid-parallel-run; isolated rerun green + full ui suite rerun
  40/40 green. Machine-load flake, no product delta — recorded, not
  registered (local-only, never a CI run).

## 5. Consecutive-green log

| #   | Run                                                                                | Scope                                 | Result                                      |
| --- | ---------------------------------------------------------------------------------- | ------------------------------------- | ------------------------------------------- |
| 1   | [43470190293](https://github.com/shubhanand021-EXA/talos/actions/runs/43470190293) | full CI on PR head (bounded-wait fix) | success (all required + convention jobs)    |
| 2   | [43471461944](https://github.com/shubhanand021-EXA/talos/actions/runs/43471461944) | full CI (probe 2/5)                   | success                                     |
| 3   | [43472688329](https://github.com/shubhanand021-EXA/talos/actions/runs/43472688329) | full CI (probe 3/5)                   | success                                     |
| 4   | [43473877472](https://github.com/shubhanand021-EXA/talos/actions/runs/43473877472) | full CI (probe 4/5)                   | success                                     |
| 5   | [43475066953](https://github.com/shubhanand021-EXA/talos/actions/runs/43475066953) | full CI (probe 5/5)                   | success (rust-core, stress x2, audit green) |
| --- | ---                                                                                | -----                                 | ------                                      |

(5 consecutive full-CI greens on the branch recorded here before merge;
any rerun obeys flake policy — at most one failed-job rerun per run.)

## 6. Secondary cures (#41 / #34)

- #41 (ToolsPage validation-hint race): root cause — the `describeTool`
  effect overwrites `paramsText` with the manifest template whenever it
  resolves, racing the test's typing under parallel CI load. Deterministic
  fix (test-only, no product change): explicit `waitFor` on the template
  value BEFORE typing; after it lands `selectedId` is stable so no further
  overwrite is possible. 10/10 local passes.
- #34 (`Install just` curl 403): `audit`-job step hardened — bounded retry
  ×3 with 15s/30s backoff, then fallback `cargo install just --version
1.59.0 --locked` (pinned, crates.io trust path), final `just --version`
  proves the probe prerequisite either way. Single install site (only the
  `audit` job shells to just.systems; `just` is a presence probe only).

## 7. FLAKES/EVIDENCE deltas

- Register rows #38/#43 (drain class) → FIXED by this prompt (cure =
  sqlx 0.9.0 close rewrite, proven §§2–4); rows #41 (deterministic test)
  and #34 (bounded retry + pinned fallback) → FIXED. Evidence comments
  appended to issues #38/#41/#43/#34; issues closed on merge.
- `docs/EVIDENCE.md`: new entry with upstream root cause (#3217/#3952),
  reproducer before/after, and the cure.

## 8. Deviations / UNKNOWNs

- No Docker/WSL on this Windows box: the 2-CPU Linux container step was
  covered by (a) local 2-core affinity loop 20/20, (b) the 5-green CI
  requirement which runs the full suite + `stress-shutdown` on ubuntu
  (2-CPU) ×5. Documented, not hidden.
- `app.test.tsx` 5s timeout in one local parallel run (see §4): load flake,
  green on isolated + full rerun. Not a CI occurrence → no register row.
- Diff: ~160 non-lockfile insertions across 9 files + 2 lockfiles (cap is
  ~600 excluding lockfile — within cap).
- UNKNOWN: none new. The 010 UNKNOWN (stalled worker vs leaked checkout) is
  now RESOLVED: fabricated-permit premature return + stuck worker return
  (source-verified above).

## 9. Commit SHA + CI URLs + status

| Item   | Value                                                                                                                                                  |
| ------ | ------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Commit | `fix(runtime): upgrade sqlx 0.9 to cure upstream pool-close race #3217; deterministic reproducer [Prompt-013]` (`5f35ca6` + `43291e1` + streak probes) |
| Branch | `fix/sqlx09-drain-cure-prompt-013` (from `df5df4c`)                                                                                                    |
| CI     | PR #48: 5 consecutive full-CI greens (§5, runs 43470190293 → 43475066953); pre-streak red fixed forward (no rerun needed)                              |
| Status | **GREEN — ready to merge**                                                                                                                             |
