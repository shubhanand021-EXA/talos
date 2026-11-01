//! Durable task-queue persistence: queue entries, attempts, transitions,
//! cancellation requests, and retry metadata.
//!
//! Current state lives in `runtime_tasks`; every mutation appends to
//! `runtime_transitions` / `runtime_attempts` and to the hash-chained
//! `event_log` so the evidence ledger stays the source of truth.

use crate::ledger::append_event;
use crate::{EvidenceDb, GENESIS_HASH};
use talos_common::{now_iso, Result, TalosError};

fn bad(what: &str) -> TalosError {
    TalosError::Storage(what.to_owned())
}

/// Durable queue entry (mirrors the `runtime_tasks` row).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct RuntimeTaskRow {
    /// Task identifier.
    pub id: String,
    /// Human-readable title.
    pub title: String,
    /// Optional description.
    pub description: Option<String>,
    /// `low` | `normal` | `high` | `critical`.
    pub priority: String,
    /// Lifecycle status (see `agent_runtime` state machine).
    pub status: String,
    /// Completed attempts so far.
    pub attempts: i64,
    /// Maximum attempts before terminal `failed`.
    pub max_attempts: i64,
    /// Per-attempt timeout in milliseconds.
    pub timeout_ms: i64,
    /// Base backoff in milliseconds.
    pub backoff_ms: i64,
    /// Deterministic mock scenario.
    pub scenario: String,
    /// Last error detail, if any.
    pub last_error: Option<String>,
    /// Next eligible retry timestamp (RFC 3339), if backed off.
    pub next_retry_at: Option<String>,
    /// Creation timestamp.
    pub created_at: String,
    /// Last-transition timestamp.
    pub updated_at: String,
    /// Optional per-task model spend cap in USD (Batch 3 LLM gateway).
    pub max_spend_usd: Option<f64>,
    /// Parent task id for delegated sub-tasks (`None` for roots).
    /// Batch 3 multi-agent hierarchy (Supervisor -> specialists).
    pub parent_task_id: Option<String>,
    /// Execution fidelity (`mock` | `live` | `verified`).
    /// Batch 4: never upgraded without the matching verification.
    pub fidelity: String,
}

/// One attempt journal row.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct RuntimeAttemptRow {
    /// Row id.
    pub id: i64,
    /// Owning task.
    pub task_id: String,
    /// 1-based attempt number.
    pub attempt: i64,
    /// Attempt start.
    pub started_at: String,
    /// Attempt end, if finished.
    pub finished_at: Option<String>,
    /// `started` | `succeeded` | `failed_retryable` | `failed_fatal` | `timed_out` | `canceled`.
    pub outcome: String,
    /// Error detail, if any.
    pub error: Option<String>,
}

/// One transition journal row.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct RuntimeTransitionRow {
    /// Row id.
    pub id: i64,
    /// Owning task.
    pub task_id: String,
    /// Previous status.
    pub from_status: String,
    /// New status.
    pub to_status: String,
    /// Human reason.
    pub reason: String,
    /// Transition timestamp.
    pub created_at: String,
}

/// Priority weight for queue ordering (`critical` first).
#[must_use]
pub fn priority_weight(priority: &str) -> i64 {
    match priority {
        "critical" => 4,
        "high" => 3,
        "normal" => 2,
        "low" => 1,
        _ => 0,
    }
}

/// Parameters for [`enqueue_task`] (keeps the function arity low).
#[derive(Debug, Clone)]
pub struct EnqueueParams<'a> {
    /// Task identifier.
    pub id: &'a str,
    /// Title.
    pub title: &'a str,
    /// Optional description.
    pub description: Option<&'a str>,
    /// Priority.
    pub priority: &'a str,
    /// Attempt budget.
    pub max_attempts: i64,
    /// Per-attempt timeout.
    pub timeout_ms: i64,
    /// Backoff base.
    pub backoff_ms: i64,
    /// Scenario.
    pub scenario: &'a str,
    /// Parent task id for delegated sub-tasks (`None` for roots).
    pub parent_task_id: Option<&'a str>,
}

/// One node of a parent-child task tree (Batch 3 delegation hierarchy).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskTreeNode {
    /// Task row at this level.
    pub task: RuntimeTaskRow,
    /// Direct children (oldest first), each with its own subtree.
    pub children: Vec<TaskTreeNode>,
}

/// Insert a new queue entry in `queued` state plus its journal rows.
///
/// Validates identifiers and bounds; rejects duplicates.
pub async fn enqueue_task(db: &EvidenceDb, params: EnqueueParams<'_>) -> Result<RuntimeTaskRow> {
    let id = params.id;
    let title = params.title;
    let description = params.description;
    let priority = params.priority;
    let max_attempts = params.max_attempts;
    let timeout_ms = params.timeout_ms;
    let backoff_ms = params.backoff_ms;
    let scenario = params.scenario;
    if id.trim().is_empty() || id.len() > 128 {
        return Err(bad("runtime task id must be 1..=128 chars"));
    }
    if title.trim().is_empty() || title.len() > 256 {
        return Err(bad("runtime task title must be 1..=256 chars"));
    }
    if !matches!(priority, "low" | "normal" | "high" | "critical") {
        return Err(bad("unknown priority"));
    }
    if !(1..=10).contains(&max_attempts) {
        return Err(bad("max_attempts must be 1..=10"));
    }
    if !(100..=3_600_000).contains(&timeout_ms) {
        return Err(bad("timeout_ms out of range"));
    }
    if let Some(parent) = params.parent_task_id {
        if parent.trim().is_empty() || parent.len() > 128 {
            return Err(bad("parent_task_id must be 1..=128 chars"));
        }
        if parent == id {
            return Err(bad("task cannot be its own parent"));
        }
        if get_task(db, parent).await?.is_none() {
            return Err(bad("unknown parent task"));
        }
    }
    let now = now_iso();
    sqlx::query(
        "INSERT INTO runtime_tasks
         (id, title, description, priority, status, attempts, max_attempts,
          timeout_ms, backoff_ms, scenario, parent_task_id, created_at, updated_at)
         VALUES (?, ?, ?, ?, 'queued', 0, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(title)
    .bind(description)
    .bind(priority)
    .bind(max_attempts)
    .bind(timeout_ms)
    .bind(backoff_ms)
    .bind(scenario)
    .bind(params.parent_task_id)
    .bind(&now)
    .bind(&now)
    .execute(db.pool())
    .await
    .map_err(|e| bad(&format!("enqueue task: {e}")))?;
    sqlx::query(
        "INSERT INTO runtime_transitions (task_id, from_status, to_status, reason, created_at)
         VALUES (?, '', 'queued', 'submitted', ?)",
    )
    .bind(id)
    .bind(&now)
    .execute(db.pool())
    .await
    .map_err(|e| bad(&format!("journal submit: {e}")))?;
    append_event(
        db,
        crate::NewEvent {
            id: format!("task-{id}-queued-{}", uuid::Uuid::new_v4()),
            kind: "task.transition".to_owned(),
            payload_json: serde_json::json!({
                "task_id": id, "from": "", "to": "queued", "reason": "submitted",
                "parent_task_id": params.parent_task_id,
            }),
        },
    )
    .await?;
    get_task(db, id)
        .await?
        .ok_or_else(|| bad("task missing after enqueue"))
}

/// List direct children of one task (oldest first, bounded).
pub async fn list_children(db: &EvidenceDb, parent_id: &str) -> Result<Vec<RuntimeTaskRow>> {
    let rows = sqlx::query_as::<_, RuntimeTaskRow>(
        "SELECT * FROM runtime_tasks WHERE parent_task_id = ? ORDER BY created_at ASC, id ASC LIMIT 100",
    )
    .bind(parent_id)
    .fetch_all(db.pool())
    .await
    .map_err(|e| bad(&format!("list children: {e}")))?;
    Ok(rows)
}

/// Depth of one task in the delegation hierarchy (roots are depth 1).
///
/// Walks `parent_task_id` links up to 16 hops; missing parents count as roots.
pub async fn task_depth(db: &EvidenceDb, task_id: &str) -> Result<usize> {
    let mut depth = 1_usize;
    let mut current = task_id.to_owned();
    for _ in 0..16 {
        let Some(row) = get_task(db, &current).await? else {
            break;
        };
        match row.parent_task_id {
            Some(parent) => {
                depth += 1;
                current = parent;
            }
            None => break,
        }
    }
    Ok(depth)
}

/// Build the parent-child subtree rooted at `root_id` (depth-first).
pub fn get_task_tree<'a>(
    db: &'a EvidenceDb,
    root_id: &'a str,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<TaskTreeNode>> + Send + 'a>> {
    Box::pin(async move {
        let root = get_task(db, root_id)
            .await?
            .ok_or_else(|| bad("unknown task"))?;
        let mut node = TaskTreeNode {
            task: root,
            children: Vec::new(),
        };
        for child in list_children(db, root_id).await? {
            node.children.push(get_task_tree(db, &child.id).await?);
        }
        Ok(node)
    })
}

/// Fetch one queue entry by id.
pub async fn get_task(db: &EvidenceDb, id: &str) -> Result<Option<RuntimeTaskRow>> {
    let row = sqlx::query_as::<_, RuntimeTaskRow>("SELECT * FROM runtime_tasks WHERE id = ?")
        .bind(id)
        .fetch_optional(db.pool())
        .await
        .map_err(|e| bad(&format!("get task: {e}")))?;
    Ok(row)
}

/// List all tasks, newest first (bounded).
pub async fn list_tasks(db: &EvidenceDb, limit: i64) -> Result<Vec<RuntimeTaskRow>> {
    let limit = limit.clamp(1, 1000);
    let rows = sqlx::query_as::<_, RuntimeTaskRow>(
        "SELECT * FROM runtime_tasks ORDER BY created_at DESC, id DESC LIMIT ?",
    )
    .bind(limit)
    .fetch_all(db.pool())
    .await
    .map_err(|e| bad(&format!("list tasks: {e}")))?;
    Ok(rows)
}

/// List runnable queue entries ordered by priority then age.
///
/// Includes `queued` entries whose backoff has elapsed; caps at `limit`.
pub async fn list_queue(db: &EvidenceDb, limit: i64, now: &str) -> Result<Vec<RuntimeTaskRow>> {
    let limit = limit.clamp(1, 1000);
    let rows = sqlx::query_as::<_, RuntimeTaskRow>(
        "SELECT * FROM runtime_tasks
         WHERE status = 'queued' AND (next_retry_at IS NULL OR next_retry_at <= ?)
         ORDER BY
           CASE priority WHEN 'critical' THEN 4 WHEN 'high' THEN 3
                        WHEN 'normal' THEN 2 WHEN 'low' THEN 1 ELSE 0 END DESC,
           created_at ASC, id ASC
         LIMIT ?",
    )
    .bind(now)
    .bind(limit)
    .fetch_all(db.pool())
    .await
    .map_err(|e| bad(&format!("list queue: {e}")))?;
    Ok(rows)
}

/// Record a validated status transition (journal + ledger + row update).
pub async fn record_transition(
    db: &EvidenceDb,
    task_id: &str,
    from: &str,
    to: &str,
    reason: &str,
) -> Result<()> {
    // Recovery-only transitions bypass this check via `force_transition`.
    if let Err(detail) = agent_lifecycle_check(from, to) {
        return Err(bad(&detail));
    }
    apply_transition(db, task_id, from, to, reason).await
}

/// Record a transition without lifecycle validation.
///
/// Restart recovery only: crash-interrupted tasks (e.g. `planning`, which has
/// no legal edge back to `queued`) must still reach a re-runnable state. Every
/// write `record_transition` performs (row update, `runtime_transitions`
/// journal row, `task.transition` ledger event) is preserved — only the
/// state-machine gate is skipped, and the `reason` always names the recovery.
pub async fn force_transition(
    db: &EvidenceDb,
    task_id: &str,
    from: &str,
    to: &str,
    reason: &str,
) -> Result<()> {
    apply_transition(db, task_id, from, to, reason).await
}

/// Lifecycle gate mirror (kept in sync with `agent_runtime::lifecycle`).
///
/// `evidence_store` cannot depend on `agent_runtime` (the dependency runs the
/// other way), so the legal edges are mirrored here. A desktop test
/// (`recovery_terminal_set_matches_lifecycle`) asserts the terminal sets
/// agree; legal edges are covered by transition tests on both sides.
fn agent_lifecycle_check(from: &str, to: &str) -> std::result::Result<(), String> {
    let legal = matches!(
        (from, to),
        ("queued", "planning" | "canceling")
            | (
                "planning",
                "executing" | "waiting_policy" | "failed" | "canceling"
            )
            | (
                "executing",
                "waiting_policy"
                    | "waiting_evidence"
                    | "waiting_compensation"
                    | "completed"
                    | "failed"
                    | "timed_out"
                    | "canceling",
            )
            | (
                "waiting_policy",
                "executing" | "failed" | "canceling" | "failed_needs_manual_review",
            )
            | (
                "waiting_evidence",
                "executing" | "failed" | "canceling" | "failed_needs_manual_review",
            )
            | (
                "waiting_compensation",
                "executing" | "failed" | "failed_needs_manual_review" | "canceling",
            )
            | ("canceling", "canceled")
            | (
                "failed" | "failed_needs_manual_review" | "timed_out" | "canceled",
                "queued",
            )
    );
    if legal {
        Ok(())
    } else {
        Err(format!("invalid transition {from} -> {to}"))
    }
}

/// Shared transition writer (row update + journal + ledger event).
async fn apply_transition(
    db: &EvidenceDb,
    task_id: &str,
    from: &str,
    to: &str,
    reason: &str,
) -> Result<()> {
    let now = now_iso();
    let updated = sqlx::query(
        "UPDATE runtime_tasks SET status = ?, updated_at = ? WHERE id = ? AND status = ?",
    )
    .bind(to)
    .bind(&now)
    .bind(task_id)
    .bind(from)
    .execute(db.pool())
    .await
    .map_err(|e| bad(&format!("transition update: {e}")))?;
    if updated.rows_affected() != 1 {
        return Err(bad("transition precondition failed (status changed?)"));
    }
    sqlx::query(
        "INSERT INTO runtime_transitions (task_id, from_status, to_status, reason, created_at)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(task_id)
    .bind(from)
    .bind(to)
    .bind(reason)
    .bind(&now)
    .execute(db.pool())
    .await
    .map_err(|e| bad(&format!("journal transition: {e}")))?;
    append_event(
        db,
        crate::NewEvent {
            id: format!("task-{task_id}-{to}-{}", uuid::Uuid::new_v4()),
            kind: "task.transition".to_owned(),
            payload_json: serde_json::json!({
                "task_id": task_id, "from": from, "to": to, "reason": reason,
            }),
        },
    )
    .await?;
    Ok(())
}

/// Begin an attempt (increments the counter, journals `started`).
pub async fn begin_attempt(db: &EvidenceDb, task_id: &str) -> Result<i64> {
    let task = get_task(db, task_id)
        .await?
        .ok_or_else(|| bad("unknown task"))?;
    let attempt = task.attempts + 1;
    let now = now_iso();
    sqlx::query("UPDATE runtime_tasks SET attempts = ?, updated_at = ? WHERE id = ?")
        .bind(attempt)
        .bind(&now)
        .bind(task_id)
        .execute(db.pool())
        .await
        .map_err(|e| bad(&format!("bump attempts: {e}")))?;
    sqlx::query(
        "INSERT INTO runtime_attempts (task_id, attempt, started_at, outcome)
         VALUES (?, ?, ?, 'started')",
    )
    .bind(task_id)
    .bind(attempt)
    .bind(&now)
    .execute(db.pool())
    .await
    .map_err(|e| bad(&format!("journal attempt: {e}")))?;
    Ok(attempt)
}

/// Finish an attempt with an outcome and optional error.
pub async fn finish_attempt(
    db: &EvidenceDb,
    task_id: &str,
    attempt: i64,
    outcome: &str,
    error: Option<&str>,
) -> Result<()> {
    let now = now_iso();
    sqlx::query(
        "UPDATE runtime_attempts SET finished_at = ?, outcome = ?, error = ?
         WHERE task_id = ? AND attempt = ?",
    )
    .bind(&now)
    .bind(outcome)
    .bind(error)
    .bind(task_id)
    .bind(attempt)
    .execute(db.pool())
    .await
    .map_err(|e| bad(&format!("finish attempt: {e}")))?;
    if let Some(message) = error {
        sqlx::query("UPDATE runtime_tasks SET last_error = ?, updated_at = ? WHERE id = ?")
            .bind(message)
            .bind(&now)
            .bind(task_id)
            .execute(db.pool())
            .await
            .map_err(|e| bad(&format!("record error: {e}")))?;
    }
    Ok(())
}

/// Set the execution fidelity of one task (`mock` | `live` | `verified`).
///
/// Fidelity only ever reflects completed verification: callers must pass
/// the weakest fidelity their evidence supports (see
/// `talos_common::check_fidelity_claim`).
pub async fn set_task_fidelity(db: &EvidenceDb, task_id: &str, fidelity: &str) -> Result<()> {
    if !matches!(fidelity, "mock" | "live" | "verified") {
        return Err(bad("fidelity must be mock|live|verified"));
    }
    sqlx::query("UPDATE runtime_tasks SET fidelity = ?, updated_at = ? WHERE id = ?")
        .bind(fidelity)
        .bind(now_iso())
        .bind(task_id)
        .execute(db.pool())
        .await
        .map_err(|e| bad(&format!("set fidelity: {e}")))?;
    Ok(())
}

/// Set the next eligible retry timestamp (backoff).
pub async fn set_next_retry(db: &EvidenceDb, task_id: &str, next_retry_at: &str) -> Result<()> {
    sqlx::query("UPDATE runtime_tasks SET next_retry_at = ?, updated_at = ? WHERE id = ?")
        .bind(next_retry_at)
        .bind(now_iso())
        .bind(task_id)
        .execute(db.pool())
        .await
        .map_err(|e| bad(&format!("set retry: {e}")))?;
    Ok(())
}

/// Request cancellation (idempotent insert; execution acknowledges later).
pub async fn request_cancellation(db: &EvidenceDb, task_id: &str) -> Result<()> {
    if get_task(db, task_id).await?.is_none() {
        return Err(bad("unknown task"));
    }
    sqlx::query(
        "INSERT OR IGNORE INTO runtime_cancellations (task_id, requested_at)
         VALUES (?, ?)",
    )
    .bind(task_id)
    .bind(now_iso())
    .execute(db.pool())
    .await
    .map_err(|e| bad(&format!("request cancel: {e}")))?;
    Ok(())
}

/// True when a cancellation was requested and not yet completed.
pub async fn cancellation_pending(db: &EvidenceDb, task_id: &str) -> Result<bool> {
    let row: Option<(i64,)> = sqlx::query_as(
        "SELECT 1 FROM runtime_cancellations
         WHERE task_id = ? AND completed = 0",
    )
    .bind(task_id)
    .fetch_optional(db.pool())
    .await
    .map_err(|e| bad(&format!("cancel check: {e}")))?;
    Ok(row.is_some())
}

/// Mark a cancellation acknowledged and/or completed.
pub async fn mark_cancellation(
    db: &EvidenceDb,
    task_id: &str,
    acknowledged: bool,
    completed: bool,
) -> Result<()> {
    sqlx::query(
        "UPDATE runtime_cancellations SET acknowledged = ?, completed = ?
         WHERE task_id = ?",
    )
    .bind(i64::from(acknowledged))
    .bind(i64::from(completed))
    .bind(task_id)
    .execute(db.pool())
    .await
    .map_err(|e| bad(&format!("mark cancel: {e}")))?;
    Ok(())
}

/// List transitions for one task (oldest first).
pub async fn list_transitions(db: &EvidenceDb, task_id: &str) -> Result<Vec<RuntimeTransitionRow>> {
    let rows = sqlx::query_as::<_, RuntimeTransitionRow>(
        "SELECT * FROM runtime_transitions WHERE task_id = ? ORDER BY id ASC",
    )
    .bind(task_id)
    .fetch_all(db.pool())
    .await
    .map_err(|e| bad(&format!("list transitions: {e}")))?;
    Ok(rows)
}

/// List attempts for one task (oldest first).
pub async fn list_attempts(db: &EvidenceDb, task_id: &str) -> Result<Vec<RuntimeAttemptRow>> {
    let rows = sqlx::query_as::<_, RuntimeAttemptRow>(
        "SELECT * FROM runtime_attempts WHERE task_id = ? ORDER BY attempt ASC",
    )
    .bind(task_id)
    .fetch_all(db.pool())
    .await
    .map_err(|e| bad(&format!("list attempts: {e}")))?;
    Ok(rows)
}

/// True when the evidence hash chain contains at least one `task.transition`
/// entry (proves transitions are ledger-backed, not memory-only).
pub async fn transitions_ledgered(db: &EvidenceDb) -> Result<bool> {
    let row: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM event_log WHERE kind = 'task.transition'")
            .fetch_one(db.pool())
            .await
            .map_err(|e| bad(&format!("ledger probe: {e}")))?;
    Ok(row.0 > 0)
}

/// Genesis hash accessor for tests (re-exported constant lives in `lib.rs`).
#[allow(dead_code)]
pub fn genesis() -> &'static str {
    GENESIS_HASH
}
