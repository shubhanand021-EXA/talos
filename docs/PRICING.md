# Talos Unit Costs (P9 cost transparency)

Our cost transparency: the list-price rates Talos multiplies by ledgered
token counts. Last verified **2026-11-14** against the provider pricing
pages below. Costs in the ledger are **estimates from list prices**
(ADR 0006) — the ±10% accuracy harness (`cost_tracker --test
cost_accuracy`) pins our pre-run projections to ledgered actuals.

## Model unit costs (USD per 1k tokens)

| Provider  | Model                    | Input / 1k | Output / 1k | Source            |
| --------- | ------------------------ | ---------: | ----------: | ----------------- |
| openai    | gpt-4o-mini              |    0.00015 |      0.0006 | OpenAI pricing    |
| openai    | gpt-4o                   |     0.0025 |        0.01 | OpenAI pricing    |
| openai    | o3-mini                  |     0.0011 |      0.0044 | OpenAI pricing    |
| anthropic | claude-3-5-haiku         |     0.0008 |       0.004 | Anthropic pricing |
| anthropic | claude-sonnet-4-20250514 |      0.003 |       0.015 | Anthropic pricing |
| ollama    | llama3 (local)           |        0.0 |         0.0 | Local — always $0 |

Source: OpenAI pricing (`https://openai.com/api/pricing`) and Anthropic
pricing (`https://www.anthropic.com/pricing`), retrieved 2026-11-14.
Catalog: `crates/llm_gateway/src/registry.rs` (`default_catalog`).

## Non-LLM unit costs (USD)

| Resource     | Formula                             |
| ------------ | ----------------------------------- |
| Tool exec    | duration × $0.0000001/ms (~$0.36/h) |
| Browser      | duration × $0.0000002/ms (~$0.72/h) |
| Computer-use | actions × $0.0005/action            |

Rates: `crates/cost_tracker/src/pricing.rs`. Attribution over
`usage_records` (LLM) + `cost_records` (everything else).

## Estimation method (`context-budget-v1`)

Pre-run estimate = `ceil(prompt_chars / 4)` prompt tokens × catalog list
price + the per-model historical mean completion tokens from the
`usage_records` ledger × catalog list price, plus duration/action legs at
the rates above. Pure and deterministic — no network, no telemetry.
Local/Ollama models always estimate exactly $0.

## ±10% guarantee scope

The guarantee covers the recorded fixture suite
(`crates/cost_tracker/tests/fixtures/cost_accuracy.json`): aggregate
`|estimate − actual| / actual ≤ 10%`, enforced by the audit swarm
(`TelemetryAgent` cost-accuracy check). Live accuracy depends on how well
the historical mean predicts your next completion; per-model drift beyond
the 5% warn line is filed as a calibration work package — the gate is
never widened silently.

## Charter note

Every local feature is free forever (P10, D7). Paid = cloud/hosted
only; local inference via Ollama is $0 and stays $0.
