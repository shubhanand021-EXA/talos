//! Real cost tracking with per-task / agent / model / tool attribution.
//!
//! Pricing inputs:
//! - LLM calls: `tokens × model price` from the gateway catalog
//!   ([`llm_cost_usd`], same linear formula as `llm_gateway`).
//! - Tool executions: `duration × compute rate` ([`tool_cost_usd`]).
//! - Browser sessions: `duration × browser rate` ([`browser_cost_usd`]).
//! - Computer-use: `action count × per-action rate`
//!   ([`computer_use_cost_usd`]).
//!
//! Attribution aggregates over two durable sources: `usage_records` (LLM)
//! and `cost_records` (tools/browser/computer-use). Budget enforcement
//! compares cumulative spend against per-task caps (`runtime_tasks.
//! max_spend_usd`) and a configurable session cap, emitting a warning at
//! 80% of any limit.

mod budget;
mod metrics;
mod pricing;

pub use budget::{
    check_child_budget, flag_child_overspend, get_child_reservation_spend, get_remaining_budget,
    reserve_budget, settle_budget, total_task_spend, BudgetCheck, BudgetDecision, BudgetTracker,
    ChildBudgetVerdict, ReservationId, SessionBudget,
};
pub use metrics::{
    cost_breakdown, operational_metrics, percentile, CostBreakdown, CostSlice, FailureReason,
    LatencyStats, ModelLatency, OperationalMetrics, ToolStats,
};
pub use pricing::{
    browser_cost_usd, computer_use_cost_usd, llm_cost_usd, tool_cost_usd, CostType,
    BROWSER_RATE_PER_MS, COMPUTER_USE_RATE_PER_ACTION, COMPUTE_RATE_PER_MS,
};
