//! Talos headless CLI (`talos`).
//!
//! Scriptable access to the same policy broker and evidence ledger the
//! desktop app uses. Every mutating path evaluates through
//! [`policy_broker::DefaultPolicyEngine`] first — the CLI cannot bypass any
//! safety gate.
//!
//! Output contract: `--format json` emits one JSON object per line (JSONL)
//! on stdout with the event types `task_started`, `cost_estimate`,
//! `policy_evaluated`, `tool_invoked`, `evidence_recorded`, `task_completed`,
//! `task_failed`, and `cost_update`. `--format text` renders the same
//! outcomes for humans. `talos run --estimate` emits only `cost_estimate`
//! and exits before execution (no spend, no ledger writes).
//!
//! Exit codes: `0` = success, `1` = failure, `2` = policy denied
//! (including a pre-run estimate that exceeds `--budget`).
//!
//! Exit codes: `0` = success, `1` = failure, `2` = policy denied.

mod commands;
mod db;
mod output;

pub use commands::{
    audit_run, config_get, config_set, doctor_report, estimate_run_cost, evidence_verify, mcp_add,
    mcp_list, mcp_remove, provider_add, provider_list, provider_remove, run_task, skill_list,
    skill_publish, skill_validate, task_cancel, task_get, task_list, AuditReport, DoctorReport,
    EvidenceReport, RunOutcome, EXIT_FAILURE, EXIT_POLICY_DENIED, EXIT_SUCCESS,
};
pub use db::{open_shared_db, CliDb};
pub use output::{emit_jsonl, print_text, OutputFormat};

/// Process-global test lock for `TALOS_DATA_DIR` isolation.
///
/// The override is process-global: parallel tests must not interleave a
/// set/open/remove sequence. Integration and unit tests hold this across
/// every `open_shared_db` call made under a scratch `TALOS_DATA_DIR`.
/// A Tokio mutex (not `std`) so guards may be held across `.await`.
pub static TEST_ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Process exit code for success.
pub const EXIT_SUCCESS_I32: i32 = 0;
/// Process exit code for failure.
pub const EXIT_FAILURE_I32: i32 = 1;
/// Process exit code for policy denial.
pub const EXIT_POLICY_DENIED_I32: i32 = 2;
