//! Foundational SQLite evidence store (append-only).
//!
//! Ledger verification arrives later; this crate only guarantees
//! durable, hash-linked inserts plus recency listings.

mod checkpoints;
mod cost;
mod crypto;
mod db;
mod ledger;
mod llm;
mod models;
pub mod recovery;
mod repo;
mod runtime;
mod skills;
pub mod storage;

pub use checkpoints::{
    latest_checkpoint, maybe_checkpoint_task, record_task_checkpoint, TaskCheckpoint,
    CHECKPOINT_KIND, DEFAULT_CHECKPOINT_EVERY, WORKSPACE_DIGEST_FILE_CAP,
};
pub use cost::{
    get_active_reservation_for_child, get_budget_reservation, insert_budget_reservation,
    insert_cost_record, list_recent_cost_records, settle_budget_reservation, sum_all_cost_records,
    sum_task_cost_records, BudgetReservationRow, CostRecordRow, NewBudgetReservation,
    NewCostRecord,
};
pub use crypto::{
    classify_bytes, classify_file, decrypt_bytes, decrypt_file_to, encrypt_bytes,
    encrypt_file_in_place, load_or_create_key, restrict_file, wipe_file, DatabaseKey, FileKind,
    KeyState, ENVELOPE_MAGIC, ENVELOPE_VERSION, KEY_NAME, KEY_SERVICE, SQLITE_MAGIC,
};
pub use db::EvidenceDb;
pub use ledger::{
    append_event, attach_evidence_to_claim, chain_hash, create_claim, create_compensation_plan,
    evaluate_claim_status, get_action_payload, get_integrity_report, has_compensation_plan,
    has_evidence_for_task, list_compensation_plans, list_plan_steps, list_recent_claims,
    list_recent_log, record_action_request, transition_compensation_plan,
    transition_compensation_step, transition_evidence_status, verify_chain, verify_evidence_chain,
    ChainReport, IntegrityReport,
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
    is_terminal as is_terminal_status, mark_clean_shutdown, mark_dirty_shutdown,
    run_startup_recovery, was_dirty_shutdown, RecoveryReport, TaskRecovery, TERMINAL_STATUSES,
};
pub use repo::{
    count_evidence_by_kind, hash_payload, insert_claim, insert_event, insert_evidence,
    latest_evidence_by_kind, list_recent_events, list_recent_evidence,
};
pub use runtime::{
    begin_attempt, cancellation_pending, enqueue_task, finish_attempt, force_transition, get_task,
    get_task_tree, list_attempts, list_children, list_queue, list_tasks, list_transitions,
    mark_cancellation, priority_weight, record_transition, request_cancellation, set_next_retry,
    set_task_fidelity, task_depth, transitions_ledgered, EnqueueParams, RuntimeAttemptRow,
    RuntimeTaskRow, RuntimeTransitionRow, TaskTreeNode,
};
pub use skills::{
    create_skill, create_skill_version, deprecate_stale_skills, get_skill, latest_skill_for_task,
    list_skill_versions, list_skills, needs_review, next_version, record_skill_use,
    should_deprecate, skill_diff, success_rate, update_skill_status, NewSkill, SkillDiffLine,
    SkillRow, SKILL_DEPRECATE_MIN_USES, SKILL_REVIEW_MIN_USES, SKILL_REVIEW_THRESHOLD,
    SKILL_STALE_DAYS, SKILL_STATUSES,
};
pub use storage::{
    default_app_data_dir, ensure_layout, list_backups_in, prune_backups_in, resolve_app_data_dir,
    BackupInfo, RestoreReport, StorageLayout, APP_DIR_NAME, BACKUP_RETENTION, DB_FILE_NAME,
};

/// Genesis hash anchoring both chains (`event_log` and evidence `seq`).
pub const GENESIS_HASH: &str = "GENESIS";
