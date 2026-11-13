# Talos P3 Performance Contract (Prompt 012)

Measured 2026-11-13 on `win32/x64/node-v26.13.1` (this dev box) via
`node scripts/perf-harness.mjs --runs 25` (deterministic: fixed seeds, mock
data) plus `topology.perf.test.ts` (real production code, jsdom).
Env tags ride every number; fidelity tags ride every claim.

## Measured vs P3 target

| P3 target          | Measured (this box)                                                                 | Verdict                                                     |
| ------------------ | ----------------------------------------------------------------------------------- | ----------------------------------------------------------- |
| ≤3s cold start p95 | spawn-floor proxy p95 ≈ 124–131ms (`perf.spawnP95Ms`, mock)                         | PROXY-ONLY — real Tauri launch unmeasured; WP-012C          |
| ≤100ms interaction | topology build+filter p95 0.563ms, feed commit p95 0.105ms (real code); proxy 0.2ms | PASS at unit scale; device trial is WP-012C                 |
| 60fps / ≤8ms frame | frame-sim proxy p95 0.009–0.013ms (mock CPU sim, not GPU)                           | PROXY PASS + runtime guard (auto lite-mode); device WP-012C |
| ≤600MB idle RAM    | node proxy RSS ≈ 44–45MB (`perf.idleRssMB`, mock)                                   | PROXY PASS; real app measurement is WP-012C                 |
| ≤60MB installers   | dist 1,837,941 bytes (real); installer unmeasurable without a signed build          | MISS (unmeasured, documented exclusion); see below          |
| auto lite-mode     | governor + HUD indicator + override shipped, unit-tested                            | PASS                                                        |

No number above is faked: mock proxies are tagged `fidelity: mock` in
`docs/evidence/perf-harness-latest.json`; real-code numbers come from vitest.
Misses are documented WPs, not widened budgets.

## What the budgets gate

`.talos/baselines.json` `perf.*` entries gate the harness proxies with
headroom (WARN) and hard failure (FAIL → CI exit 1 via `--check` and the
`perf` CI job). Proxy budgets mirror P3 magnitudes (100ms interaction,
8ms frame, 600MB RAM) so a regression loud enough to threaten P3 trips the
proxy first. `ui.coldStartP95Ms` / `ui.installerBytes` are `referenceOnly`:
recorded method, no fake gate.

## Graceful degrade

Frame governor (`lite-mode.ts`, pure + tested): 3 consecutive sub-30fps
windows → lite; 5 consecutive ≥50fps windows → full (hysteresis); HUD
override (auto/lite/full) always wins. Lite reduces starfield 800→150,
disables bloom, DPR→1, no antialias, pulses capped at 6 and frozen, orbit
static. Palette, typography, layout unchanged (identity preserved).

## Installer size (local-measured)

CI cannot sign or bundle (documented exclusion in `docs/CI.md`). Local
method: `pnpm build:desktop`, then compare the installer bytes against the
60MB P3 target and record the number in the release report. First
measurement owed before launch (WP-012C).
