# Report 016 — Pre-run cost estimates, ±10% accuracy gate, published unit costs (Prompt 016)

- Date: 2026-11-14 (UTC)
- Agent: OpenCode (sole engineer)
- Batch: 2 / Prompt: 016 (P9 cost transparency)
- Read order followed: `AGENTS.md` → `VISION.md` (P9) → `docs/TELEMETRY.md` →
  ADR-0006 (`docs/adr/0006-llm-gateway-byok.md`) → `crates/cost_tracker` +
  `llm_gateway` → `docs/CI.md` → latest `reports/014-migration-ui.md`
- Branch: `feat/cost-estimates-prompt-016` (from `main` `7af271b`)

## 0. Scope note (009/011/013/014 precedent)

~1350 insertions across 26 files — over the ~600 target. The diff is one
atomic gate: estimator without the harness/surfaces is unverifiable, and
the harness without fixtures + swarm wiring enforces nothing. Splitting
would half-build the P9 criterion the prompt exists to ship, so the
overage is documented, not split. Follow-up WP-016A below carries the
optional live-key calibration (explicitly out of scope).

## 1. Estimator method (`context-budget-v1`)

`crates/cost_tracker/src/estimate.rs` (new, pure, deterministic, no I/O):

- Prompt tokens = `ceil(prompt_chars / 4)` — the ~4 chars/token English
  density shared with the context-budget layer (documented heuristic).
- Completion tokens = caller-supplied prediction; callers SHOULD use
  `historical_completion_average` (per-model mean completion tokens from
  the `usage_records` ledger; `FALLBACK_COMPLETION_TOKENS = 512` when the
  model has no history).
- Tool / browser / computer-use legs reuse the exact `pricing.rs` rates,
  so the only error source is token projection, never arithmetic.
- Local models (Ollama catalog prices are $0) always estimate exactly $0.
- `CostEstimate` (serde `camelCase`) carries provider/model, token counts,
  per-leg USD, total USD, and the method label.

## 2. UI/CLI surface (before spend, deny intact)

- Desktop IPC `estimate_task_cost` (`apps/desktop/src-tauri/src/telemetry.rs`,
  registered in `lib.rs`): prices the planned initial model call from the
  live gateway catalog (default `openai/gpt-4o-mini`), prompt chars =
  title + description + 1200-char orchestrator seed; no spend, no ledger
  writes. Contracts: `PreRunCostEstimateSchema` / `EstimateTaskCostInputSchema`
  (`ops.ts`; `PreRun…` prefix avoids the approval-flow `CostEstimate`
  collision caught by `contracts build`), `estimateTaskCost` IPC name,
  pinned command list updated.
- UI: Agents task-submit flow shows a debounced pre-run estimate line
  (`≈$X · N prompt + M completion tokens (model) · budget deny intact`)
  via bridge `estimateTaskCost`; Tauri-only, backend-unavailable safe.
- CLI: `talos run --estimate` emits a single `cost_estimate` JSONL event
  and exits before execution (no task enqueue, no ledger writes; exit 2
  when `--budget` is set and the plan alone exceeds it). Normal `talos run`
  now emits `cost_estimate` second (after `task_started`, before the policy
  gate) and denies with exit 2 when the estimate exceeds `--budget` — the
  existing budget-deny rule applied before traffic (previously the CLI
  recorded the budget but never enforced it).
- Execution paths untouched: `submit_task` still enforces per-task caps in
  the gateway (`BudgetExceeded`, zero provider contact).

## 3. Accuracy harness results (aggregate 2.19% ≤ 10%)

`crates/cost_tracker/tests/cost_accuracy.rs` +
`tests/fixtures/cost_accuracy.json` (6 recorded-style fixtures, no live
paid calls; `prompt_chars` is measured representative text,
`predicted_completion_tokens` simulates the pre-run historical mean):

| Fixture                        | Estimate | Actual   | Rel err |
| ------------------------------ | -------- | -------- | ------- |
| gpt-4o-mini coding turn        | 0.000916 | 0.000932 | 1.69%   |
| gpt-4o vision review           | 0.008563 | 0.008350 | 2.54%   |
| claude-3-5-haiku browse + read | 0.003130 | 0.003160 | 0.94%   |
| claude-sonnet computer-use fix | 0.024450 | 0.024000 | 1.87%   |
| o3-mini reasoning pass         | 0.006903 | 0.007150 | 3.46%   |
| ollama llama3 local            | 0.000000 | 0.000000 | exact-0 |

`aggregate_relative_error=0.021916` → gate green with 4.6× headroom.
Zero-actual (local) fixtures assert exactly $0 and are excluded from the
relative aggregate. Missed 10% would be a calibration WP, never a widened
gate (assert message says so verbatim).

## 4. Gate wiring (swarm WARN/FAIL)

`TelemetryAgent` §11 (`tools/audit/src/agents/telemetry.ts`): static
presence checks (estimator, fixtures, `docs/PRICING.md` with per-1k prices

- sources) + executes the harness (`cargo test -p cost_tracker --test
cost_accuracy -- --nocapture`), parses `aggregate_relative_error`: >10%
  → critical, >5% → warning, pass → info + `costAccuracy.aggregateError`
  metric. Live `pnpm audit:swarm`: `TelemetryAuditAgent … (critical 0,
warnings 0)`, metrics include `costAccuracy.aggregateError=0.021916`.
  Audit unit tests extended (fixture files + `cost-accuracy` info check).

## 5. docs/PRICING.md delta

New: per-model input/output unit-cost table (6 catalog models, sources =
provider pricing pages, verified 2026-11-14), non-LLM rates, estimation
method, ±10% fixture-suite scope, local/Ollama = $0 + P10/D7 charter
note. Linked from README (Model providers → Costs) + `docs/TELEMETRY.md`
(Operations dashboard section).

## 6. Follow-up WPs

- WP-016A (optional, env-gated): live accuracy calibration with a real
  key — run the estimator against live completions locally, compare to
  ledgered actuals, file per-model calibration if drift >5%. Never in CI,
  never committed. (Prompt-declared optional; nothing owed.)
- No variance WP owed: fixture aggregate 2.19% is inside the gate; no
  model class misses ±10% on fixtures.

## 7. Commit SHA + CI URLs + status

| Item   | Value                                                                                                |
| ------ | ---------------------------------------------------------------------------------------------------- |
| Commit | `feat(cost): pre-run estimates, ±10% accuracy gate, published unit costs [Prompt-016]` (SHA on push) |
| Branch | `feat/cost-estimates-prompt-016` (from `7af271b`)                                                    |
| CI     | PR run URL + status before merge (attempt-1 red → fix → green counts as the 2 bounded attempts)      |

### Gate evidence (exit codes, durations where measured)

- `cargo test -p cost_tracker` → exit 0 (16 unit + accuracy harness
  `aggregate_relative_error=0.021916`).
- `cargo test --workspace` → exit 0 (all crates green, incl. new
  `talos_cli` estimate/deny tests, 14/14).
- `cargo clippy --workspace --all-targets -- -D warnings` → exit 0 (51s).
- `cargo fmt --all -- --check` (crates + src-tauri) → exit 0.
- `cargo check --lib` + `cargo clippy --lib` (src-tauri) → exit 0.
- `cargo test --lib` (src-tauri) → loader crash `0xc0000139`
  `STATUS_ENTRYPOINT_NOT_FOUND` before any test executes; IDENTICAL on
  stashed unmodified main → pre-existing ENVIRONMENT finding (missing
  system DLL entrypoint on this box), not a gate — CI `desktop-check`
  runs fmt + clippy + check only, never tests.
- `pnpm install --frozen-lockfile` → exit 0; `pnpm-lock.yaml`
  byte-identical (`git diff` empty).
- `pnpm lint` (prettier + all workspace lints) → exit 0.
- `pnpm typecheck` → exit 0. `pnpm test` → exit 0 (contracts 114,
  audit 32, browser-use 57, ui 42 files/202 tests).
- `pnpm build` → exit 0. `pnpm audit` → 1 moderate pre-existing
  (minimatch via eslint toolchain); `--audit-level critical` exit 0 —
  zero dependency change, upstream advisory data only.
- `pnpm audit:swarm` → exit 0, 0 criticals (privacy 8 + deps 1 warnings
  pre-existing); `TelemetryAuditAgent (critical 0, warnings 0)`.

Final line: commit SHA + CI run URL + green status recorded on push.
