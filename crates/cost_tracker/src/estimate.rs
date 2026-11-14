//! Pre-run cost estimation (P9: estimates within ±10% of actual).
//!
//! Estimation method (`context-budget-v1`):
//! - Prompt tokens: `ceil(prompt_chars / CHARS_PER_TOKEN)`. The ~4
//!   chars/token density for English text is shared with the context-budget
//!   layer; it is a documented heuristic, not a measurement.
//! - Completion tokens: caller-supplied prediction. Callers SHOULD derive it
//!   from [`historical_completion_average`] (per-model mean completion tokens
//!   from the `usage_records` ledger). The accuracy harness pins the
//!   aggregate error of this method to ≤10% on recorded fixtures.
//! - Tool / browser / computer-use legs reuse the exact [`crate::pricing`]
//!   rates, so the only error source is token projection, never arithmetic.
//! - Local models (Ollama catalog prices are $0) always estimate $0 exactly.
//!
//! Pure and deterministic: no I/O, no network. Ledger access lives in the
//! single async helper below; everything else is a pure function.

use evidence_store::EvidenceDb;
use talos_common::Result;

/// Characters per projected prompt token (English-text density heuristic).
pub const CHARS_PER_TOKEN: f64 = 4.0;

/// Fallback predicted completion tokens when the ledger has no history.
pub const FALLBACK_COMPLETION_TOKENS: u32 = 512;

/// Method label stamped on every estimate (surfaced in UI + CLI JSONL).
pub const ESTIMATE_METHOD: &str = "context-budget-v1: ceil(chars/4) prompt projection x catalog list prices + per-model historical completion mean";

/// One planned non-LLM cost leg.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedToolUse {
    /// Tool id (informational; cost is duration × compute rate).
    pub tool_id: String,
    /// Expected duration in milliseconds.
    pub duration_ms: u64,
}

/// Full pre-run plan feeding the estimator.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskCostPlan {
    /// Provider id (e.g. `openai`, `ollama`).
    pub provider_id: String,
    /// Model id (e.g. `gpt-4o-mini`).
    pub model_id: String,
    /// Catalog list price: USD per 1k input tokens.
    pub cost_per_1k_input: f64,
    /// Catalog list price: USD per 1k output tokens.
    pub cost_per_1k_output: f64,
    /// Projected prompt size in characters (caller assembles context).
    pub prompt_chars: u64,
    /// Predicted completion tokens (prefer the ledger historical mean).
    pub expected_completion_tokens: u32,
    /// Planned tool executions.
    pub tools: Vec<PlannedToolUse>,
    /// Planned browser automation in milliseconds.
    pub browser_ms: u64,
    /// Planned computer-use actions.
    pub computer_actions: u64,
}

/// A pre-run cost estimate (no spend incurred; budget deny still applies).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CostEstimate {
    /// Provider id the estimate was priced for.
    pub provider_id: String,
    /// Model id the estimate was priced for.
    pub model_id: String,
    /// Projected prompt tokens.
    pub prompt_tokens: u32,
    /// Predicted completion tokens.
    pub completion_tokens: u32,
    /// Projected LLM cost in USD.
    pub llm_cost_usd: f64,
    /// Projected tool-execution cost in USD.
    pub tool_cost_usd: f64,
    /// Projected browser-automation cost in USD.
    pub browser_cost_usd: f64,
    /// Projected computer-use cost in USD.
    pub computer_use_cost_usd: f64,
    /// Total projected cost in USD.
    pub total_cost_usd: f64,
    /// Estimation method label ([`ESTIMATE_METHOD`]).
    pub method: String,
}

/// Project prompt tokens from a character count (ceil division).
#[must_use]
pub fn project_prompt_tokens(prompt_chars: u64) -> u32 {
    ((prompt_chars as f64) / CHARS_PER_TOKEN)
        .ceil()
        .min(u32::MAX as f64) as u32
}

/// Estimate a task plan. Pure, deterministic, no I/O.
#[must_use]
pub fn estimate_task_cost(plan: &TaskCostPlan) -> CostEstimate {
    let prompt_tokens = project_prompt_tokens(plan.prompt_chars);
    let llm_cost_usd = crate::pricing::llm_cost_usd(
        prompt_tokens,
        plan.expected_completion_tokens,
        plan.cost_per_1k_input,
        plan.cost_per_1k_output,
    );
    let tool_cost_usd: f64 = plan
        .tools
        .iter()
        .map(|t| crate::pricing::tool_cost_usd(t.duration_ms))
        .sum();
    let browser_cost_usd = crate::pricing::browser_cost_usd(plan.browser_ms);
    let computer_use_cost_usd = crate::pricing::computer_use_cost_usd(plan.computer_actions);
    CostEstimate {
        provider_id: plan.provider_id.clone(),
        model_id: plan.model_id.clone(),
        prompt_tokens,
        completion_tokens: plan.expected_completion_tokens,
        llm_cost_usd,
        tool_cost_usd,
        browser_cost_usd,
        computer_use_cost_usd,
        total_cost_usd: llm_cost_usd + tool_cost_usd + browser_cost_usd + computer_use_cost_usd,
        method: ESTIMATE_METHOD.to_owned(),
    }
}

/// Mean of a sample, or `None` when empty (no history to learn from).
#[must_use]
pub fn mean_u32(samples: &[u32]) -> Option<f64> {
    if samples.is_empty() {
        return None;
    }
    Some(samples.iter().map(|s| f64::from(*s)).sum::<f64>() / samples.len() as f64)
}

/// Relative error `|estimate - actual| / actual`, or `None` when the actual
/// is zero (zero-spend legs must estimate exactly zero instead).
#[must_use]
pub fn relative_error(estimate_usd: f64, actual_usd: f64) -> Option<f64> {
    if actual_usd <= 0.0 {
        return None;
    }
    Some((estimate_usd - actual_usd).abs() / actual_usd)
}

/// Per-model mean completion tokens from the `usage_records` ledger.
/// `None` when the model has no recorded calls (caller falls back to
/// [`FALLBACK_COMPLETION_TOKENS`]).
pub async fn historical_completion_average(
    db: &EvidenceDb,
    provider_id: &str,
    model_id: &str,
) -> Result<Option<f64>> {
    let avg: Option<f64> = sqlx::query_scalar(
        "SELECT AVG(CAST(completion_tokens AS REAL)) FROM usage_records
          WHERE provider_id = ? AND model_id = ?",
    )
    .bind(provider_id)
    .bind(model_id)
    .fetch_one(db.pool())
    .await
    .map_err(|e| talos_common::TalosError::Storage(format!("completion average: {e}")))?;
    Ok(avg.filter(|m| m.is_finite() && *m >= 0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan() -> TaskCostPlan {
        TaskCostPlan {
            provider_id: "openai".to_owned(),
            model_id: "gpt-4o-mini".to_owned(),
            cost_per_1k_input: 0.00015,
            cost_per_1k_output: 0.0006,
            prompt_chars: 9500,
            expected_completion_tokens: 600,
            tools: vec![PlannedToolUse {
                tool_id: "fs.read".to_owned(),
                duration_ms: 2000,
            }],
            browser_ms: 0,
            computer_actions: 0,
        }
    }

    #[test]
    fn projects_prompt_tokens_by_ceil() {
        assert_eq!(project_prompt_tokens(0), 0);
        assert_eq!(project_prompt_tokens(1), 1);
        assert_eq!(project_prompt_tokens(9500), 2375);
        assert_eq!(project_prompt_tokens(9501), 2376);
    }

    #[test]
    fn estimate_matches_gateway_formula_and_is_deterministic() {
        let first = estimate_task_cost(&plan());
        let second = estimate_task_cost(&plan());
        assert_eq!(first, second);
        // 2375 prompt × 0.00015/1k + 600 completion × 0.0006/1k.
        let expected_llm = (2375.0 / 1000.0) * 0.00015 + (600.0 / 1000.0) * 0.0006;
        assert!((first.llm_cost_usd - expected_llm).abs() < 1e-12);
        assert!((first.tool_cost_usd - 0.0002).abs() < 1e-12);
        assert!((first.total_cost_usd - (expected_llm + 0.0002)).abs() < 1e-12);
        assert_eq!(first.method, ESTIMATE_METHOD);
    }

    #[test]
    fn local_models_estimate_zero() {
        let mut local = plan();
        local.provider_id = "ollama".to_owned();
        local.model_id = "llama3".to_owned();
        local.cost_per_1k_input = 0.0;
        local.cost_per_1k_output = 0.0;
        let estimate = estimate_task_cost(&local);
        assert_eq!(estimate.llm_cost_usd, 0.0);
    }

    #[test]
    fn error_and_mean_helpers() {
        assert!((relative_error(1.05, 1.0).unwrap() - 0.05).abs() < 1e-12);
        assert_eq!(relative_error(0.0, 0.0), None);
        assert_eq!(mean_u32(&[]), None);
        assert_eq!(mean_u32(&[400, 600]), Some(500.0));
    }

    #[tokio::test]
    async fn historical_average_empty_then_seeded() {
        let db = EvidenceDb::connect_memory().await.expect("memory db");
        assert_eq!(
            historical_completion_average(&db, "openai", "gpt-4o-mini")
                .await
                .unwrap(),
            None
        );
        for (id, completion) in [("u-1", 400), ("u-2", 600)] {
            evidence_store::insert_usage_record(
                &db,
                &evidence_store::NewUsageRecord {
                    id,
                    task_id: "t-1",
                    agent_id: "a-1",
                    provider_id: "openai",
                    model_id: "gpt-4o-mini",
                    prompt_tokens: 100,
                    completion_tokens: completion,
                    total_tokens: 100 + completion,
                    estimated_cost_usd: 0.001,
                    latency_ms: 10,
                },
            )
            .await
            .unwrap();
        }
        assert_eq!(
            historical_completion_average(&db, "openai", "gpt-4o-mini")
                .await
                .unwrap(),
            Some(500.0)
        );
    }
}
