//! Startup crash recovery for the durable task queue.
//!
//! On `connect_file`, [`run_startup_recovery`] reconciles crash-interrupted
//! tasks with the persisted ledger:
//! - `executing` + checkpoint → `queued` (`resumed_from_checkpoint`, plus a
//!   `task_resumed` event naming the checkpoint)
//! - `executing` without checkpoint → `failed` (`crash_recovery_failed`)
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
    /// True when the previous process exited without a clean shutdown.
    pub dirty_shutdown: bool,
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

/// Read the shutdown flag (`dirty` = previous process died mid-flight).
///
/// Missing row (first run) or a missing table (pre-migration database that
/// never tracked shutdowns) counts as clean.
pub async fn was_dirty_shutdown(db: &EvidenceDb) -> Result<bool> {
    let flag: Option<String> = match sqlx::query_scalar("SELECT flag FROM shutdown_state LIMIT 1")
        .fetch_optional(db.pool())
        .await
    {
        Ok(flag) => flag,
        Err(err) if err.to_string().contains("no such table") => return Ok(false),
        Err(err) => return Err(bad(&format!("shutdown flag read: {err}"))),
    };
    Ok(flag.as_deref() == Some("dirty"))
}

/// Mark this startup as unclean (called before recovery runs).
pub async fn mark_dirty_shutdown(db: &EvidenceDb) -> Result<()> {
    sqlx::query(
        "INSERT INTO shutdown_state (flag, updated_at) VALUES ('dirty', ?) ON CONFLICT(flag) DO UPDATE SET updated_at = excluded.updated_at",
    )
    .bind(now_iso())
    .execute(db.pool())
    .await
    .map_err(|err| bad(&format!("shutdown flag write: {err}")))?;
    Ok(())
}

/// Mark an orderly shutdown (called on the desktop exit path).
pub async fn mark_clean_shutdown(db: &EvidenceDb) -> Result<()> {
    sqlx::query("DELETE FROM shutdown_state WHERE flag = 'dirty'")
        .execute(db.pool())
        .await
        .map_err(|err| bad(&format!("shutdown flag clear: {err}")))?;
    Ok(())
}

/// Reconcile crash-interrupted tasks and record the startup integrity verdict.
///
/// Never fails a startup: a broken chain is reported in the returned
/// [`RecoveryReport`] (plus a critical log line) and recovery proceeds.
/// `executing` tasks with a checkpoint resume (`queued`,
/// `resumed_from_checkpoint`); without one they fail
/// (`crash_recovery_failed`).
pub async fn run_startup_recovery(db: &EvidenceDb) -> Result<RecoveryReport> {
    let dirty = was_dirty_shutdown(db).await?;
    mark_dirty_shutdown(db).await?;
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
        let (to_status, reason, force, resumed_checkpoint) = match task.status.as_str() {
            "queued" => ("queued", "restart_recovery_seen_queued", false, None),
            "planning" => ("queued", "requeued_after_restart", true, None),
            // Executing tasks resume from their checkpoint when one exists;
            // without a checkpoint there is nothing safe to resume from.
            "executing" => match crate::checkpoints::latest_checkpoint(db, &task.id).await? {
                Some(checkpoint) => (
                    "queued",
                    "resumed_from_checkpoint",
                    true,
                    Some(checkpoint.id),
                ),
                None => ("failed", "crash_recovery_failed", false, None),
            },
            "waiting_policy" => ("failed", "approval_lost_on_restart", false, None),
            "waiting_evidence" | "waiting_compensation" => {
                ("failed", "interrupted_by_restart", false, None)
            }
            "canceling" => ("canceled", "cancel_completed_on_restart", false, None),
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
        if let Some(checkpoint_id) = resumed_checkpoint {
            append_event(
                db,
                NewEvent {
                    id: format!("task-{}-resumed-{}", task.id, uuid::Uuid::new_v4()),
                    kind: "task_resumed".to_owned(),
                    payload_json: serde_json::json!({
                        "task_id": task.id,
                        "checkpoint_id": checkpoint_id,
                        "reason": reason,
                    }),
                },
            )
            .await?;
            events_recorded += 1;
        }
        recovered.push(TaskRecovery {
            task_id: task.id,
            from_status: task.status,
            to_status: to_status.to_owned(),
            reason: reason.to_owned(),
        });
    }

    let resumed = recovered
        .iter()
        .filter(|r| r.reason == "resumed_from_checkpoint")
        .count();
    append_event(
        db,
        NewEvent {
            id: format!("startup-recovery-{}", uuid::Uuid::new_v4()),
            kind: "startup_recovery".to_owned(),
            payload_json: serde_json::json!({
                "recovered": recovered.len(),
                "resumed": resumed,
                "dirty_shutdown": dirty,
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
        dirty_shutdown: dirty,
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
        assert_eq!(by_id["t-exec"].reason, "crash_recovery_failed");
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
        // Recovery is journaled + ledgered (t-exec failed without a
        // checkpoint; t-evidence keeps the legacy interrupted reason).
        let transitions = crate::list_transitions(&db, "t-exec").await.expect("lists");
        assert!(transitions
            .iter()
            .any(|t| t.reason == "crash_recovery_failed"));
        let transitions = crate::list_transitions(&db, "t-evidence")
            .await
            .expect("lists");
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
        assert_eq!(report.recovered[0].reason, "crash_recovery_failed");
        let task = crate::get_task(&db, "t-restart")
            .await
            .expect("gets")
            .expect("exists");
        assert_eq!(task.status, "failed");
    }

    #[tokio::test]
    async fn dirty_flag_detects_unclean_shutdown() {
        let (_dir, db) = file_db("recovery-dirty").await;
        // Fresh database: no flag row yet → clean.
        assert!(!was_dirty_shutdown(&db).await.expect("reads"));
        mark_dirty_shutdown(&db).await.expect("marks");
        assert!(was_dirty_shutdown(&db).await.expect("reads"));
        mark_clean_shutdown(&db).await.expect("cleans");
        assert!(!was_dirty_shutdown(&db).await.expect("reads"));
        // Recovery observes a clean shutdown on first run and leaves the
        // dirty flag set for the running process.
        let report = run_startup_recovery(&db).await.expect("recovers");
        assert!(!report.dirty_shutdown);
        assert!(was_dirty_shutdown(&db).await.expect("reads"));
        mark_clean_shutdown(&db).await.expect("cleans");
    }

    #[tokio::test]
    async fn executing_task_with_checkpoint_resumes() {
        let (_dir, db) = file_db("recovery-resume").await;
        enqueue(&db, "t-resume").await;
        set_status(&db, "t-resume", "executing").await;
        crate::record_task_checkpoint(&db, "t-resume", None)
            .await
            .expect("checkpoints");
        let report = run_startup_recovery(&db).await.expect("recovers");
        let resumed = report
            .recovered
            .iter()
            .find(|r| r.task_id == "t-resume")
            .expect("recovered");
        assert_eq!(resumed.to_status, "queued");
        assert_eq!(resumed.reason, "resumed_from_checkpoint");
        let task = crate::get_task(&db, "t-resume")
            .await
            .expect("gets")
            .expect("exists");
        assert_eq!(task.status, "queued");
        // The resume names its checkpoint in the ledger.
        let log = crate::list_recent_log(&db, 50).await.expect("lists");
        assert!(log.iter().any(|e| e.kind == "task_resumed"));
    }

    #[tokio::test]
    async fn simulated_crash_restart_recovers_with_dirty_flag() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("crash.db");
        // First boot: clean recovery, dirty flag left set.
        let db = EvidenceDb::connect_file(&path).await.expect("connects");
        let first = run_startup_recovery(&db).await.expect("recovers");
        assert!(!first.dirty_shutdown);
        enqueue(&db, "t-crash").await;
        set_status(&db, "t-crash", "executing").await;
        crate::record_task_checkpoint(&db, "t-crash", None)
            .await
            .expect("checkpoints");
        drop(db);
        // No mark_clean_shutdown: this is the crash. Reopen = next boot.
        let db = EvidenceDb::connect_file(&path).await.expect("reconnects");
        let report = run_startup_recovery(&db).await.expect("recovers");
        assert!(report.dirty_shutdown);
        let resumed = report
            .recovered
            .iter()
            .find(|r| r.task_id == "t-crash")
            .expect("recovered");
        assert_eq!(resumed.reason, "resumed_from_checkpoint");
        let task = crate::get_task(&db, "t-crash")
            .await
            .expect("gets")
            .expect("exists");
        assert_eq!(task.status, "queued");
        mark_clean_shutdown(&db).await.expect("cleans");
        assert!(!was_dirty_shutdown(&db).await.expect("reads"));
    }
}
