//! P9 accuracy harness: pre-run estimate vs ledgered actual on recorded
//! fixtures (no live paid calls in CI; see `fixtures/cost_accuracy.json`).
//!
//! Gate: aggregate `Σ|estimate − actual| / Σactual` (non-zero legs only)
//! must stay ≤ `gate_max_aggregate_error` (10%). Zero-actual fixtures
//! (local models) must estimate exactly $0. Live accuracy with a real key
//! is OPTIONAL and env-gated — never committed, never in CI.

use std::collections::HashMap;

use cost_tracker::{
    browser_cost_usd, computer_use_cost_usd, estimate_task_cost, llm_cost_usd, relative_error,
    tool_cost_usd, PlannedToolUse, TaskCostPlan,
};

#[derive(Debug, serde::Deserialize)]
struct Fixture {
    name: String,
    provider_id: String,
    model_id: String,
    cost_per_1k_input: f64,
    cost_per_1k_output: f64,
    prompt_chars: u64,
    prompt_tokens_actual: u32,
    predicted_completion_tokens: u32,
    completion_tokens_actual: u32,
    tool_ms: HashMap<String, u64>,
    browser_ms: u64,
    computer_actions: u64,
}

#[derive(Debug, serde::Deserialize)]
struct Suite {
    gate_max_aggregate_error: f64,
    fixtures: Vec<Fixture>,
}

fn load_suite() -> Suite {
    let raw = include_str!("fixtures/cost_accuracy.json");
    serde_json::from_str(raw).expect("fixtures parse")
}

#[test]
fn estimate_within_gate_on_recorded_fixtures() {
    let suite = load_suite();
    assert!(
        !suite.fixtures.is_empty(),
        "fixture suite must not be empty"
    );
    let mut abs_error_sum = 0.0;
    let mut actual_sum = 0.0;
    println!(
        "{:<38} {:>12} {:>12} {:>8}",
        "fixture", "estimate", "actual", "rel_err"
    );
    for fixture in &suite.fixtures {
        let tools: Vec<PlannedToolUse> = fixture
            .tool_ms
            .iter()
            .map(|(tool_id, duration_ms)| PlannedToolUse {
                tool_id: tool_id.clone(),
                duration_ms: *duration_ms,
            })
            .collect();
        let plan = TaskCostPlan {
            provider_id: fixture.provider_id.clone(),
            model_id: fixture.model_id.clone(),
            cost_per_1k_input: fixture.cost_per_1k_input,
            cost_per_1k_output: fixture.cost_per_1k_output,
            prompt_chars: fixture.prompt_chars,
            expected_completion_tokens: fixture.predicted_completion_tokens,
            tools,
            browser_ms: fixture.browser_ms,
            computer_actions: fixture.computer_actions,
        };
        let estimate = estimate_task_cost(&plan);
        let actual_llm = llm_cost_usd(
            fixture.prompt_tokens_actual,
            fixture.completion_tokens_actual,
            fixture.cost_per_1k_input,
            fixture.cost_per_1k_output,
        );
        let actual_tracked: f64 = fixture
            .tool_ms
            .values()
            .map(|ms| tool_cost_usd(*ms))
            .sum::<f64>()
            + browser_cost_usd(fixture.browser_ms)
            + computer_use_cost_usd(fixture.computer_actions);
        let actual = actual_llm + actual_tracked;
        match relative_error(estimate.total_cost_usd, actual) {
            Some(rel) => {
                println!(
                    "{:<38} {:>12.6} {:>12.6} {:>7.2}%",
                    fixture.name,
                    estimate.total_cost_usd,
                    actual,
                    rel * 100.0
                );
                abs_error_sum += (estimate.total_cost_usd - actual).abs();
                actual_sum += actual;
            }
            None => {
                // Zero-actual legs (local models) must estimate exactly $0.
                println!(
                    "{:<38} {:>12.6} {:>12.6} {:>8}",
                    fixture.name, estimate.total_cost_usd, actual, "exact-0"
                );
                assert_eq!(
                    estimate.total_cost_usd, 0.0,
                    "zero-actual fixture must estimate $0: {}",
                    fixture.name
                );
            }
        }
    }
    let aggregate = abs_error_sum / actual_sum;
    println!("aggregate_relative_error={aggregate:.6}");
    assert!(
        aggregate <= suite.gate_max_aggregate_error,
        "P9 gate breached: aggregate {aggregate:.4} > max {:.4} — file a calibration WP, do NOT widen the gate",
        suite.gate_max_aggregate_error
    );
}
