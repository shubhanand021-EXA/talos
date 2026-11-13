# Report 012 — Deterministic P3 harness, lite-mode, CI budgets (Prompt 012)

- Date: 2026-11-13 (UTC)
- Agent: OpenCode (sole engineer)
- Batch: 2 / Prompt: 012 (P3 performance contract)
- Read order followed: `AGENTS.md` → `VISION.md` (P3) → `docs/DECISIONS.md` →
  `reports/001-*` … `011-*` → `docs/CI.md` → `.talos/baselines.json`
- Branch: `perf/void-neon-p3-prompt-012` (from `main` `175f872`)

## 0. Diff size (documented overage, 009/011 precedent)

~990 insertions + report/PROGRESS/EVIDENCE. Every file maps to an acceptance
criterion below; the constitutional core (measure + degrade + gate) cannot be
split without half-building P3, so no WP split is filed — same judgment as
009 (3885 lines) and 011 (947 lines).

## 1. Harness design

`scripts/perf-harness.mjs` (node builtins only, fixed seeds, mock data):
per-run process-spawn floor, 1000-event feed commit, 120-frame scene-update
sim (LinkFlow pulse-lerp shape over a 120-node golden-spiral graph),
algorithmic mirror of `buildLiveTopology`, node RSS, plus real
`apps/ui/dist` bytes. Emits p50/p95 over N≥20 runs, env-tagged
(os/arch/node/cpus) and fidelity-tagged (`mock` for CPU proxies — never
presented as verified). `--check` compares p95s against
`.talos/baselines.json` fail gates (exit 1 on breach, WARN on warn breach).
Output: `docs/evidence/perf-harness-latest.json` (committed sample).

## 2. Measured baselines (win32/x64/node-v26.13.1, 25 runs)

| P3 target          | Measured                                                              | Verdict                                                     |
| ------------------ | --------------------------------------------------------------------- | ----------------------------------------------------------- |
| ≤3s cold start p95 | spawn-floor proxy p95 ≈ 124–131ms (`perf.spawnP95Ms`, mock)           | PROXY-ONLY — real launch unmeasured, WP-012C                |
| ≤100ms interaction | real code: topology build+filter p95 0.563ms, feed commit p95 0.105ms | PASS at unit scale; device trial is WP-012C                 |
| 60fps / ≤8ms frame | frame-sim proxy p95 0.009–0.023ms (mock CPU, not GPU)                 | PROXY PASS + runtime guard; device WP-012C                  |
| ≤600MB idle RAM    | node proxy RSS ≈ 44MB (`perf.idleRssMB`, mock)                        | PROXY PASS; real app is WP-012C                             |
| ≤60MB installers   | dist 1,837,941 bytes (real); installer unmeasurable without signing   | MISS (documented exclusion; local method in PERFORMANCE.md) |
| auto lite-mode     | governor + HUD indicator + override shipped, unit-tested              | PASS                                                        |

Full mapping, headroom rationale, and installer method in `docs/PERFORMANCE.md`.
`ui.coldStartP95Ms` / `ui.installerBytes` are `referenceOnly` zeros —
placeholders, never claims (a missed target is a documented WP, not a lie).

## 3. Lite-mode behavior + tests

`lite-mode.ts` (pure governor): 3 consecutive sub-30fps windows → lite;
5 consecutive ≥50fps → full (hysteresis); HUD override (auto/lite/full)
always wins. Effects: starfield 800→150, bloom off, DPR→1, no antialias,
pulses capped at 6 and frozen, orbit static — palette/typography/layout
untouched. `lite-mode.test.ts`: overload triggers, recovery restores,
override wins both ways, 10k-iteration purity (no hidden state/allocation).
Wired via `SceneSettingsProvider.reportFps` (throttled FpsMeter samples),
HUD pill + select, effective-low quality in canvas/backdrop/linkflow.

## 4. Gate wiring

- `.talos/baselines.json`: 5 gated `perf.*` proxies (warn headroom, fail =
  CI exit 1) + 2 `referenceOnly` P3 placeholders.
- `PerformanceAgent` reads the harness JSON, enforces env/fidelity tags,
  records `perf.*` into swarm metrics → shared evaluator WARN/FAILs on
  breach (missing/untagged file warns, never fails).
- New `perf` CI job (ubuntu, display-free): build UI dist + harness
  `--runs 20 --check`; documented as convention-required in `docs/CI.md`.
- `agents.test.ts`: tagged-metrics recorded, untagged file rejected,
  missing file warns.

## 5. Optimizations (before/after)

None. Measurement proves headroom, not offenders: real topology build p95
0.563ms vs 100ms budget (178x), feed commit 0.105ms (952x), harness
proxies 10–1000x under fail gates. Per protocol (only what measurement
proves, no speculative rewrites), Phase D ships zero product changes.

## 6. Follow-up WPs (scoped, not built)

- WP-012C (device truth): real Tauri cold start (local launch timing),
  on-device interaction/frame/RAM trials, first signed-installer byte count
  for the 60MB target. Needs display + bundler + signing identity — none
  available in CI by documented exclusion.
- WP-011B carry-over (migration UI) untouched by this prompt.

## 7. Commit SHA + CI URLs + status

| Item   | Value                                                                                           |
| ------ | ----------------------------------------------------------------------------------------------- |
| Commit | `perf(ui): deterministic P3 harness, lite-mode, CI budgets [Prompt-012]` (SHA recorded on push) |
| Branch | `perf/void-neon-p3-prompt-012` (from `175f872`)                                                 |
| CI     | PR run URL + status recorded on push; required + convention set green                           |
| Status | **AWAITING CI** (local gates green; updated on result)                                          |
