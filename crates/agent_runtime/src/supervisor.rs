//! Supervisor: timeouts, stuck-task detection, cancellation, and
//! policy-respecting retries. Read-only except for validated transitions;
//! it never evaluates policy itself and never bypasses the broker.

use evidence_store::EvidenceDb;
use talos_common::Result;

use crate::queue::transition_task;

/// Outcome of one supervision pass.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SupervisionReport {
    /// Tasks moved to `timed_out`.
    pub timed_out: usize,
    /// Tasks moved to `canceling`.
    pub cancel_acknowledged: usize,
    /// Tasks requeued for retry.
    pub retried: usize,
}

/// Run one supervision pass over live tasks.
///
/// - Respects cancellation: pending requests move tasks toward `canceling`.
/// - Enforces timeouts: attempts running past `timeout_ms` become `timed_out`.
/// - Retries when policy allows: `failed` tasks with attempts left and an
///   elapsed backoff are requeued to `queued` (validated transition).
pub async fn supervise_once(db: &EvidenceDb, now_rfc3339: &str) -> Result<SupervisionReport> {
    let mut report = SupervisionReport::default();
    let tasks = evidence_store::list_tasks(db, 1000).await?;
    for task in tasks {
        if matches!(task.status.as_str(), "completed" | "canceled") {
            continue;
        }
        // Cancellation first: pending intent wins over progress.
        if evidence_store::cancellation_pending(db, &task.id).await? {
            if matches!(
                task.status.as_str(),
                "queued"
                    | "planning"
                    | "executing"
                    | "waiting_policy"
                    | "waiting_evidence"
                    | "waiting_compensation"
            ) {
                transition_task(db, &task.id, "canceling", "supervisor: cancel acknowledged")
                    .await?;
                report.cancel_acknowledged += 1;
            }
            continue;
        }
        // Timeout: current attempt started before now - timeout_ms.
        if matches!(
            task.status.as_str(),
            "planning"
                | "executing"
                | "waiting_policy"
                | "waiting_evidence"
                | "waiting_compensation"
        ) {
            let attempts = evidence_store::list_attempts(db, &task.id).await?;
            if let Some(current) = attempts.iter().rev().find(|a| a.finished_at.is_none()) {
                let started = current.started_at.clone();
                if elapsed_ms(&started, now_rfc3339) > task.timeout_ms {
                    evidence_store::finish_attempt(
                        db,
                        &task.id,
                        current.attempt,
                        "timed_out",
                        Some("supervisor timeout"),
                    )
                    .await?;
                    transition_task(db, &task.id, "timed_out", "supervisor: timeout").await?;
                    report.timed_out += 1;
                    continue;
                }
            }
        }
        // Retry: failed/timed_out with budget left and backoff elapsed.
        if matches!(task.status.as_str(), "failed" | "timed_out") {
            let eligible = match task.next_retry_at.as_deref() {
                Some(at) => at <= now_rfc3339,
                None => true,
            };
            if eligible && task.attempts < task.max_attempts {
                transition_task(db, &task.id, "queued", "supervisor: retry").await?;
                report.retried += 1;
            }
        }
    }
    Ok(report)
}

/// Milliseconds between two RFC 3339 timestamps (negative on parse failure).
fn elapsed_ms(started_at: &str, now_at: &str) -> i64 {
    let parse = |s: &str| chrono::DateTime::parse_from_rfc3339(s).map(|d| d.timestamp_millis());
    match (parse(started_at), parse(now_at)) {
        (Ok(a), Ok(b)) => b - a,
        _ => -1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elapsed_computes() {
        assert_eq!(
            elapsed_ms("2026-10-31T00:00:00.000Z", "2026-10-31T00:00:01.000Z"),
            1000
        );
    }
}
