# Report 015 — Real cold start + installer size: BLOCKED (Prompt 015)

- Date: 2026-11-14 (UTC)
- Agent: OpenCode (sole engineer)
- Batch: 2 / Prompt: 015 (close the two honest P3 misses, WP-012C)
- Read order followed: `AGENTS.md` → `VISION.md` (P3) → `reports/012-performance-pass.md` →
  `docs/PERFORMANCE.md` → `.talos/baselines.json` → `docs/CI.md`
- Outcome: **BLOCKED after 2 bounded fix attempts.** Failure-protocol
  `reports/015-real-perf-BLOCKED.md` + STOP. No third attempt, no budget
  change, no methodology softening. `main` untouched and green throughout.

## 1. What was proven (local, dedicated hardware — all green)

Real Tauri cold start and unsigned installer bytes were measured, gated,
and wired exactly as the prompt asked — and they PASS on hardware that
is not a loaded shared runner:

| Metric                                                                 | Local win32/x64/release (20/20, 0 failed) | P3 gate                              | Verdict                  |
| ---------------------------------------------------------------------- | ----------------------------------------- | ------------------------------------ | ------------------------ |
| Cold start p50 / p95 (attempt-2 harness: warmup + `setup_enter` split) | 961ms / 1080ms                            | p95 ≤ 3000ms (warn 2500 / fail 3000) | **PASS** (2.8x headroom) |
| Unsigned NSIS installer (`tauri build --bundles nsis --no-sign`)       | 8,762,805 bytes (8.4MB)                   | ≤ 60MB each (warn 50MB / fail 60MB)  | **PASS** (7x headroom)   |

Attribution split (every run): `setup_enter_ms` ≈ 60–70% of setup
(native webview creation), closure body ≈ 40–60ms, paint-after-setup ≈
small. Product code's own share is tens of ms. Local gates on the final
tree: `cargo fmt --check` (0), `cargo clippy --all-targets -D warnings`
(0), `cargo check --all-targets` (0), `cargo test --workspace` in
`crates/` (0), `pnpm lint` (0), `pnpm typecheck` (0), `pnpm test` (0:
40 node + 114 contracts + 57 browser-use + 202 UI), `pnpm build` (0),
`pnpm audit` (0), `pnpm audit:swarm` (0 criticals; PerformanceAgent
records `ui.coldStartP95Ms`/`ui.installerBytes` from verified JSONs).

Genuine product bugs found and proven en route (F1–F3, verified live via
CDP; deferred to a follow-up prompt, NOT merged here): F1 broken
`frontendDist` (`../ui/dist` → `../../ui/dist`); F2 deny-all IPC (empty
capability grants rejected every command; explicit 117-command
allow-list); F3 Tauri camelCase wire args (`firstPaintClientMs`; systemic
snake_case mismatch filed as WP-015B). F4 (3s bounded keychain probe +
`setup_ms` split) and F5 (`setup_enter` split + Linux sandbox env +
warmup) are measurement-robustness work, also deferred.

## 2. CI evidence (three runs, same conclusion)

Branch `perf/real-coldstart-installer-prompt-015`, PR #50
(`https://github.com/shubhanand021-EXA/talos/pull/50`). Every other job
green in all three runs; ONLY `perf` (cold-start `--check`) red:

| CI run                 | Commit                                              | ubuntu p95 (setup/run)                                                                                                                                          | windows p95                                                                                                                 | Result               |
| ---------------------- | --------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- | -------------------- |
| `43572846222` (16m28s) | `b875988`                                           | 32440.8ms                                                                                                                                                       | 3515.0ms                                                                                                                    | `perf` red ONLY      |
| `43578203959` (16m12s) | `d1cc723` (F4 keychain cap)                         | 32263.9ms (setup ≈27–31s EVERY run)                                                                                                                             | 3282.2ms (18/20 ≈450ms, setup ≈320ms; outliers 3282/2484ms)                                                                 | `perf` red ONLY      |
| `43581307109` (16m3s)  | `7add391` (F5 sandbox env + warmup + `setup_enter`) | 30871.7ms, p50 30851.6ms — sandbox var had ZERO effect (log: `measure env: WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1` printed, warmup `cold_start_ms=34189.5`) | 6377.2ms, p50 602.6ms — warmup absorbed cold-run-1; SIX mid-series spikes (2590/6377/2939/2266/2354/1742ms), all in `enter` | `perf` red BOTH legs |

Decisive attribution (`setup_enter_ms`, committed by every run):

- ubuntu: `enter` ≈ 30.1s (16/20) or ≈ 26.2s (4/20); closure body ≈ 20ms;
  paint-after-setup ≈ 700ms. The ~30s sits ENTIRELY in Tauri-internal
  window creation (Tauri v2 creates config webviews BEFORE the user
  `setup` closure — verified in `tauri-2.12.5` `app.rs` `setup()`).
  Per-launch constant (not cache-building: runs 2–20 identical), immune
  to the sandbox var. Root cause beyond the sandbox is unprovable from
  here (no Linux box; candidates — WebKit process-launch fallback
  timeouts, proxy/D-Bus stalls under xvfb — are all guesses). Guessing a
  third fix would violate "no speculative fixes."
- windows: `enter` spikes (WebView2/Edge init under CPU steal on shared
  runners). p50 602.6ms proves the product is fast; nearest-rank p95 over
  20 (= max) catches the worst steal spike (6377ms). Even linear
  interpolation would read ≈3112ms > 3000 — no honest statistic passes
  with 6 spikes in 20 runs.

## 3. Finding (not a product failure)

Shared CI runners cannot host a ≤3s cold-start p95 gate for a
WebView-based desktop app: Linux containers pay ~26–30s per launch in
native webview creation; Windows shared runners inject multi-second
WebView2-init spikes mid-series. The product itself measures 961/1080ms
locally with 2.8x headroom. Per AGENTS.md §6 (red gate on a clean
machine is a FINDING first; environment-level causes are findings, never
patched by weakening product logic), the gate is unpassable here without
dedicated runners (out of scope) or post-hoc statistic surgery
(p-hacking — refused).

## 4. Honest misses + WPs (nothing widened, nothing redefined)

- MISS (CI-side only): cold-start p95 ≤3s gated in CI. Gates UNCHANGED
  (warn 2500 / fail 3000); percentile estimator UNTOUCHED (nearest-rank).
- WP-015B (open, unchanged): systemic IPC snake_case→camelCase audit.
- WP-015C (new): CI-runnable cold-start gate — needs either dedicated
  runners with user namespaces + GPU, or a product-side change that
  removes native webview init from the critical path (out of scope for
  this batch). Re-propose F1/F2/F4/F5 fixes inside WP-015C's prompt
  WITHOUT the CI gate until the environment question is answered.
- WP-015D (new): Windows CI steal-spike tolerance — if the gate returns,
  it needs a spike-robust design decided BEFORE seeing data (e.g. more
  runs + a pre-registered estimator), never fitted post-hoc.
- Installer half of the prompt is DONE and green everywhere (8.4MB vs
  60MB, gated in CI + swarm); it re-rides with WP-015C.

## 5. Revert record (green restored)

- `main` was never touched (HEAD `2c94285` throughout; lockfile
  byte-identical in every diff).
- Branch reset to `main` (`git reset --hard 2c94285`); the three
  un-mergeable commits (`b875988`, `d1cc723`, `7add391`) are abandoned on
  the branch reflog, SHAs recorded here. This file is the ONLY change
  merged: docs-only, full-matrix CI green before merge.
- Stale `permissions/autogenerated/` cruft left untracked throughout
  (inert, out of scope).

## 6. Commit SHA + CI URLs + status

| Item                   | Value                                                                                                                 |
| ---------------------- | --------------------------------------------------------------------------------------------------------------------- |
| Commit                 | `docs(report): Prompt 015 blocked — CI runners cannot host cold-start gate [Prompt-015]` (SHA recorded on merge)      |
| Branch                 | `perf/real-coldstart-installer-prompt-015` (reset to `2c94285` + this file)                                           |
| CI (red, evidence)     | `43572846222` (16m28s) → `43578203959` (16m12s) → `43581307109` (16m3s); `perf` red every time, everything else green |
| CI (green, this merge) | report-only run URL + status recorded before merge                                                                    |
| Status                 | **BLOCKED — STOP** (2/2 fix attempts spent; no merge of red code; no budget/methodology change)                       |
