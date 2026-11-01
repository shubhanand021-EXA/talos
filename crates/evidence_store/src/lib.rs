//! Foundational SQLite evidence store (append-only).
//!
//! Ledger verification arrives later; this crate only guarantees
//! durable, hash-linked inserts plus recency listings.

mod db;
mod ledger;
mod llm;
mod models;
mod repo;
mod runtime;

pub use db::EvidenceDb;
pub use ledger::{
    append_event, attach_evidence_to_claim, chain_hash, create_claim, create_compensation_plan,
    evaluate_claim_status, get_integrity_report, has_compensation_plan, has_evidence_for_task,
    list_compensation_plans, list_plan_steps, list_recent_claims, list_recent_log,
    record_action_request, transition_compensation_plan, transition_compensation_step,
    transition_evidence_status, verify_chain, verify_evidence_chain, ChainReport, IntegrityReport,
};
pub use llm::{
    get_task_budget, hash_request_metadata, insert_model_interaction, insert_usage_record,
    list_recent_usage, set_task_budget, sum_task_spend, summarize_usage,
    summarize_usage_by_provider, ModelInteractionRow, NewModelInteraction, NewUsageRecord,
    UsageRecordRow,
};
pub use models::{
    ActionRequestRow, ClaimRow, CompensationPlanRow, CompensationStepRow, EventLogRow, EventRow,
    EvidenceRow, NewActionRecord, NewClaim, NewClaimPending, NewCompensationPlan,
    NewCompensationStep, NewEvent, NewEvidence, PolicyEvaluationRow,
};
pub use repo::{
    hash_payload, insert_claim, insert_event, insert_evidence, list_recent_events,
    list_recent_evidence,
};
pub use runtime::{
    begin_attempt, cancellation_pending, enqueue_task, finish_attempt, get_task, get_task_tree,
    list_attempts, list_children, list_queue, list_tasks, list_transitions, mark_cancellation,
    priority_weight, record_transition, request_cancellation, set_next_retry, task_depth,
    transitions_ledgered, EnqueueParams, RuntimeAttemptRow, RuntimeTaskRow, RuntimeTransitionRow,
    TaskTreeNode,
};

/// Genesis hash anchoring both chains (`event_log` and evidence `seq`).
pub const GENESIS_HASH: &str = "GENESIS";
