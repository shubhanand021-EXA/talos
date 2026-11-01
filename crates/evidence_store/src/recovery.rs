//! Startup crash recovery for the durable task queue.
//!
//! On `connect_file`, [`run_startup_recovery`] reconciles crash-interrupted
//! tasks with the persisted ledger:
//! - `executing` → `failed` (`interrupted_by_restart`)
//! - `waiting_policy` → `failed` (`approval_lost_on_restart`)
//! - `waiting_evidence` / `waiting_compensation` → `failed`
//!   (`interrupted_by_restart`)
//! - `canceling` → `canceled` (`cancel_completed_on_restart`)
//! - `planning` → `queued` (`requeued_after_restart`, via the recovery-only
//!   [`force_transition`](crate::force_transition) — `planning` has no legal
//!   edge back to `queued`, so the state-machine gate is bypassed but the
//!   journal + ledger writes are identical)
//! - `queued` → untouched (re-runnable as-is)
//!
//! Every recovery is journaled (`runtime_transitions`), ledgered
//! (`task.transition` events), and summarized in one `startup_recovery`
//! event plus a `startup_integrity_verified` / `startup_integrity_broken`
//! event. A broken chain is reported — never a startup refusal: the user
//! needs access to diagnose.
//!
//! Approval recovery limitation: [`ApprovalStore`](https://github.com/shubhanand021-EXA/talos)
//! (`crates/agent_runtime/src/approval.rs`) is in-memory, so pending approvals
//! do not survive a restart. Tasks that were `waiting_policy` are failed with
//! `approval_lost_on_restart` (never silently resumed): the user re-plans or
//! retries them explicitly. Persisting approvals is future work.

use talos_common::{now_iso, Result, TalosError};

use crate::db::EvidenceDb;
use crate::ledger::{append_event, verify_chain};
use crate::{force_transition, list_tasks, record_transition, NewEvent};

/// Terminal lifecycle states (mirrors `agent_runtime::lifecycle::is_terminal`).
///
/// Mirrored here because `evidence_store` cannot depend on `agent_runtime`.
/// A desktop integration test asserts both sets agree.
pub const TERMINAL_STATUSES: &[&str] = &[
    "completed",
    "failed",
    "failed_needs_manual_review",
    "canceled",
    "timed_out",
];

/// True for terminal statuses (no recovery needed).
#[must_use]
pub fn is_terminal(status: &str) -> bool {
    TERMINAL_STATUSES.contains(&status)
}

/// One task reconciled by startup recovery.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TaskRecovery {
    /// Recovered task id.
    pub task_id: String,
    /// Status observed at startup.
    pub from_status: String,
    /// Status written by recovery (`queued` tasks report `queued` → `queued`).
    pub to_status: String,
    /// Machine reason (`interrupted_by_restart`, ...).
    pub reason: String,
}

/// Outcome of [`run_startup_recovery`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RecoveryReport {
    /// Event-log chain verdict at startup.
    pub chain_valid: bool,
    /// Links recomputed during the startup check.
    pub chain_checked: i64,
    /// First bad sequence when the chain is broken.
    pub first_bad_seq: Option<i64>,
    /// Per-task reconciliations (empty on a clean shutdown).
    pub recovered: Vec<TaskRecovery>,
    /// Ledger events recorded by this recovery run.
    pub events_recorded: usize,
    /// When the recovery ran (RFC 3339).
    pub recovered_at: String,
}

fn bad(msg: &str) -> TalosError {
    TalosError::Storage(msg.to_owned())
}

/// Reconcile crash-interrupted tasks and record the startup integrity verdict.
///
/// Never fails a startup: a broken chain is reported in the returned
/// [`RecoveryReport`] (plus a critical log line) and recovery proceeds.
pub async fn run_startup_recovery(db: &EvidenceDb) -> Result<RecoveryReport> {
    let chain = verify_chain(db).await?;
    let mut events_recorded = 0_usize;

    let integrity_kind = if chain.valid {
        "startup_integrity_verified"
    } else {
        eprintln!(
            "[talos] CRITICAL: event-log hash chain broken at seq {:?}; recording startup_integrity_broken and continuing",
            chain.first_bad_seq
        );
        "startup_integrity_broken"
    };
    append_event(
        db,
        NewEvent {
            id: format!("startup-integrity-{}", uuid::Uuid::new_v4()),
            kind: integrity_kind.to_owned(),
            payload_json: serde_json::json!({
                "valid": chain.valid,
                "checked": chain.checked,
                "first_bad_seq": chain.first_bad_seq,
            }),
        },
    )
    .await?;
    events_recorded += 1;

    let mut recovered = Vec::new();
    // Oldest first for deterministic recovery order.
    let mut tasks = list_tasks(db, 1000).await?;
    tasks.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
    for task in tasks {
        if is_terminal(&task.status) {
            continue;
        }
        let (to_status, reason, force) = match task.status.as_str() {
            "queued" => ("queued", "restart_recovery_seen_queued", false),
            "planning" => ("queued", "requeued_after_restart", true),
            "executing" => ("failed", "interrupted_by_restart", false),
            "waiting_policy" => ("failed", "approval_lost_on_restart", false),
            "waiting_evidence" | "waiting_compensation" => {
                ("failed", "interrupted_by_restart", false)
            }
            "canceling" => ("canceled", "cancel_completed_on_restart", false),
            other => {
                return Err(bad(&format!("unknown task status at startup: {other}")));
            }
        };
        if task.status == to_status {
            // `queued` stays queued: journal the observation without a move.
            append_event(
                db,
                NewEvent {
                    id: format!("task-{}-restart-seen-{}", task.id, uuid::Uuid::new_v4()),
                    kind: "restart_recovery".to_owned(),
                    payload_json: serde_json::json!({
                        "task_id": task.id, "from": task.status, "to": to_status, "reason": reason,
                    }),
                },
            )
            .await?;
            events_recorded += 1;
        } else if force {
            force_transition(db, &task.id, &task.status, to_status, reason).await?;
            events_recorded += 1;
        } else {
            record_transition(db, &task.id, &task.status, to_status, reason).await?;
            events_recorded += 1;
        }
        recovered.push(TaskRecovery {
            task_id: task.id,
            from_status: task.status,
            to_status: to_status.to_owned(),
            reason: reason.to_owned(),
        });
    }

    append_event(
        db,
        NewEvent {
            id: format!("startup-recovery-{}", uuid::Uuid::new_v4()),
            kind: "startup_recovery".to_owned(),
            payload_json: serde_json::json!({
                "recovered": recovered.len(),
                "chain_valid": chain.valid,
            }),
        },
    )
    .await?;
    events_recorded += 1;

    Ok(RecoveryReport {
        chain_valid: chain.valid,
        chain_checked: chain.checked,
        first_bad_seq: chain.first_bad_seq,
        recovered,
        events_recorded,
        recovered_at: now_iso(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::EnqueueParams;

    async fn file_db(name: &str) -> (tempfile::TempDir, EvidenceDb) {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join(format!("{name}.db"));
        let db = EvidenceDb::connect_file(&path)
            .await
            .expect("file database connects");
        (dir, db)
    }

    async fn enqueue(db: &EvidenceDb, id: &str) {
        crate::enqueue_task(
            db,
            EnqueueParams {
                id,
                title: "recovery probe",
                description: None,
                priority: "normal",
                max_attempts: 3,
                timeout_ms: 30_000,
                backoff_ms: 500,
                scenario: "quick_success",
                parent_task_id: None,
            },
        )
        .await
        .expect("enqueue works");
    }

    async fn set_status(db: &EvidenceDb, id: &str, to: &str) {
        // Test-only staging: set the pre-crash status directly (bypasses the
        // lifecycle gate exactly like a crash would — no journal rows).
        let updated =
            sqlx::query("UPDATE runtime_tasks SET status = ? WHERE id = ? AND status = 'queued'")
                .bind(to)
                .bind(id)
                .execute(db.pool())
                .await
                .expect("stage status");
        assert_eq!(updated.rows_affected(), 1, "staged {id} -> {to}");
    }

    #[tokio::test]
    async fn clean_shutdown_recovers_nothing() {
        let (_dir, db) = file_db("recovery-clean").await;
        enqueue(&db, "task-clean").await;
        crate::record_transition(&db, "task-clean", "queued", "planning", "test")
            .await
            .expect("transitions");
        crate::record_transition(&db, "task-clean", "planning", "executing", "test")
            .await
            .expect("transitions");
        crate::record_transition(&db, "task-clean", "executing", "completed", "test")
            .await
            .expect("transitions");
        let report = run_startup_recovery(&db).await.expect("recovers");
        assert!(report.chain_valid);
        assert!(report.recovered.is_empty());
        assert_eq!(report.events_recorded, 2); // integrity + summary
    }

    #[tokio::test]
    async fn interrupted_tasks_transition_to_recoverable_states() {
        let (_dir, db) = file_db("recovery-moves").await;
        for id in [
            "t-exec",
            "t-policy",
            "t-evidence",
            "t-comp",
            "t-cancel",
            "t-plan",
            "t-queue",
        ] {
            enqueue(&db, id).await;
        }
        set_status(&db, "t-exec", "executing").await;
        set_status(&db, "t-policy", "waiting_policy").await;
        set_status(&db, "t-evidence", "waiting_evidence").await;
        set_status(&db, "t-comp", "waiting_compensation").await;
        set_status(&db, "t-cancel", "canceling").await;
        set_status(&db, "t-plan", "planning").await;

        let report = run_startup_recovery(&db).await.expect("recovers");
        assert!(report.chain_valid);
        let by_id: std::collections::HashMap<_, _> = report
            .recovered
            .iter()
            .map(|r| (r.task_id.as_str(), r))
            .collect();
        assert_eq!(by_id["t-exec"].to_status, "failed");
        assert_eq!(by_id["t-exec"].reason, "interrupted_by_restart");
        assert_eq!(by_id["t-policy"].to_status, "failed");
        assert_eq!(by_id["t-policy"].reason, "approval_lost_on_restart");
        assert_eq!(by_id["t-evidence"].reason, "interrupted_by_restart");
        assert_eq!(by_id["t-comp"].reason, "interrupted_by_restart");
        assert_eq!(by_id["t-cancel"].to_status, "canceled");
        assert_eq!(by_id["t-plan"].to_status, "queued");
        assert_eq!(by_id["t-plan"].reason, "requeued_after_restart");
        assert_eq!(by_id["t-queue"].to_status, "queued");

        // Persisted states match the report.
        for (id, to) in [
            ("t-exec", "failed"),
            ("t-policy", "failed"),
            ("t-cancel", "canceled"),
            ("t-plan", "queued"),
        ] {
            let task = crate::get_task(&db, id)
                .await
                .expect("gets")
                .expect("exists");
            assert_eq!(task.status, to, "{id}");
        }
        // Recovery is journaled + ledgered.
        let transitions = crate::list_transitions(&db, "t-exec").await.expect("lists");
        assert!(transitions
            .iter()
            .any(|t| t.reason == "interrupted_by_restart"));
    }

    #[tokio::test]
    async fn broken_chain_is_reported_not_refused() {
        let (_dir, db) = file_db("recovery-tamper").await;
        enqueue(&db, "t-live").await;
        sqlx::query("UPDATE event_log SET payload_json = ? WHERE seq = 1")
            .bind(r#"{"forged":true}"#)
            .execute(db.pool())
            .await
            .expect("tampers");
        let report = run_startup_recovery(&db).await.expect("still recovers");
        assert!(!report.chain_valid);
        assert!(report.first_bad_seq.is_some());
        // The interrupted task is still reconciled despite the broken chain.
        assert!(report.recovered.iter().any(|r| r.task_id == "t-live"));
    }

    #[tokio::test]
    async fn recovery_survives_restart() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("restart.db");
        let db = EvidenceDb::connect_file(&path).await.expect("connects");
        enqueue(&db, "t-restart").await;
        set_status(&db, "t-restart", "executing").await;
        drop(db);
        // Simulate process restart: reopen and recover.
        let db = EvidenceDb::connect_file(&path).await.expect("reconnects");
        let report = run_startup_recovery(&db).await.expect("recovers");
        assert_eq!(report.recovered.len(), 1);
        assert_eq!(report.recovered[0].reason, "interrupted_by_restart");
        let task = crate::get_task(&db, "t-restart")
            .await
            .expect("gets")
            .expect("exists");
        assert_eq!(task.status, "failed");
    }
}
