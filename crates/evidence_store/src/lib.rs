//! Foundational SQLite evidence store (append-only).
//!
//! Ledger verification arrives later; this crate only guarantees
//! durable, hash-linked inserts plus recency listings.

mod cost;
mod db;
mod ledger;
mod llm;
mod models;
pub mod recovery;
mod repo;
mod runtime;
mod skills;
pub mod storage;

pub use cost::{
    insert_cost_record, list_recent_cost_records, sum_all_cost_records, sum_task_cost_records,
    CostRecordRow, NewCostRecord,
};
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
pub use recovery::{
    is_terminal as is_terminal_status, run_startup_recovery, RecoveryReport, TaskRecovery,
    TERMINAL_STATUSES,
};
pub use repo::{
    hash_payload, insert_claim, insert_event, insert_evidence, list_recent_events,
    list_recent_evidence,
};
pub use runtime::{
    begin_attempt, cancellation_pending, enqueue_task, finish_attempt, force_transition, get_task,
    get_task_tree, list_attempts, list_children, list_queue, list_tasks, list_transitions,
    mark_cancellation, priority_weight, record_transition, request_cancellation, set_next_retry,
    task_depth, transitions_ledgered, EnqueueParams, RuntimeAttemptRow, RuntimeTaskRow,
    RuntimeTransitionRow, TaskTreeNode,
};
pub use skills::{
    create_skill, get_skill, list_skills, update_skill_status, NewSkill, SkillRow, SKILL_STATUSES,
};
pub use storage::{
    default_app_data_dir, ensure_layout, list_backups_in, prune_backups_in, resolve_app_data_dir,
    BackupInfo, RestoreReport, StorageLayout, APP_DIR_NAME, BACKUP_RETENTION, DB_FILE_NAME,
};

/// Genesis hash anchoring both chains (`event_log` and evidence `seq`).
pub const GENESIS_HASH: &str = "GENESIS";
