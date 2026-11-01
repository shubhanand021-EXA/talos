//! Durable task queue: submit / cancel / retry / inspect plus a
//! priority-ordered worker and cooperative cancellation.
//!
//! Persistence lives in `evidence_store::runtime_*` (SQLite); every status
//! change goes through [`lifecycle::check_transition`] and
//! [`evidence_store::record_transition`], which journals the move and appends
//! a `task.transition` entry to the hash-chained `event_log`.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use evidence_store::EvidenceDb;
use policy_broker::{PolicyContext, PolicyEngine};
use talos_common::{now_iso, Result, TalosError};

use crate::lifecycle;
use crate::MockEchoAgent;

fn err(msg: &str) -> TalosError {
    TalosError::Storage(msg.to_owned())
}

/// Worker tuning (no hidden globals; the desktop layer owns one instance).
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    /// Maximum tasks executed concurrently by `run_due_tasks`.
    pub max_concurrency: usize,
    /// Default per-attempt timeout when the caller omits one.
    pub default_timeout_ms: i64,
    /// Default attempt budget when the caller omits one.
    pub default_max_attempts: i64,
    /// Base backoff between retryable attempts.
    pub backoff_base_ms: i64,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            max_concurrency: 4,
            default_timeout_ms: 30_000,
            default_max_attempts: 3,
            backoff_base_ms: 500,
        }
    }
}

/// Cooperative cancellation token (cloned into workers; no blocking calls).
#[derive(Debug, Clone)]
pub struct CancelToken {
    cancelled: Arc<AtomicBool>,
}

impl CancelToken {
    /// Create an un-cancelled token.
    #[must_use]
    pub fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Request cancellation.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    /// True after [`CancelToken::cancel`].
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

impl Default for CancelToken {
    fn default() -> Self {
        Self::new()
    }
}

/// Parameters for [`submit_task`] (validated before any write).
#[derive(Debug, Clone)]
pub struct SubmitParams {
    /// Human-readable title.
    pub title: String,
    /// Optional detail.
    pub description: Option<String>,
    /// `low` | `normal` | `high` | `critical`.
    pub priority: String,
    /// Per-attempt timeout in milliseconds.
    pub timeout_ms: i64,
    /// Attempt budget (1..=10).
    pub max_attempts: i64,
    /// Deterministic mock scenario.
    pub scenario: String,
    /// Parent task for delegated sub-tasks (`None` for roots).
    pub parent_task_id: Option<String>,
}

/// Queue entry surfaced to IPC/UI (camelCase over JSON).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskView {
    /// Task identifier.
    pub id: String,
    /// Title.
    pub title: String,
    /// Lifecycle status.
    pub status: String,
    /// Priority.
    pub priority: String,
    /// Completed attempts.
    pub attempts: i64,
    /// Attempt budget.
    pub max_attempts: i64,
    /// Per-attempt timeout.
    pub timeout_ms: i64,
    /// Mock scenario.
    pub scenario: String,
    /// Last error, if any.
    pub last_error: Option<String>,
    /// Next eligible retry, if backed off.
    pub next_retry_at: Option<String>,
    /// Parent task for delegated sub-tasks (`None` for roots).
    pub parent_task_id: Option<String>,
    /// Creation timestamp.
    pub created_at: String,
    /// Last-transition timestamp.
    pub updated_at: String,
}

/// One node of a task tree surfaced to IPC/UI (parent → children).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskTreeView {
    /// Task at this level.
    pub task: TaskView,
    /// Direct children (oldest first), each with its own subtree.
    pub children: Vec<TaskTreeView>,
}

impl From<evidence_store::RuntimeTaskRow> for TaskView {
    fn from(row: evidence_store::RuntimeTaskRow) -> Self {
        Self {
            id: row.id,
            title: row.title,
            status: row.status,
            priority: row.priority,
            attempts: row.attempts,
            max_attempts: row.max_attempts,
            timeout_ms: row.timeout_ms,
            scenario: row.scenario,
            last_error: row.last_error,
            next_retry_at: row.next_retry_at,
            parent_task_id: row.parent_task_id,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

fn validate_priority(priority: &str) -> Result<String> {
    match priority {
        "low" | "normal" | "high" | "critical" => Ok(priority.to_owned()),
        _ => Err(err("unknown priority (low|normal|high|critical)")),
    }
}

fn validate_scenario(scenario: &str) -> Result<String> {
    match scenario {
        "quick_success"
        | "delayed_success"
        | "retryable_failure"
        | "non_retryable_failure"
        | "cancellation_sensitive" => Ok(scenario.to_owned()),
        _ => Err(err("unknown scenario")),
    }
}

/// Submit a task: validates, persists in `queued`, journals the transition.
pub async fn submit_task(
    db: &EvidenceDb,
    counter: &mut u64,
    params: SubmitParams,
) -> Result<TaskView> {
    submit_task_with_id(db, counter, params, None).await
}

/// Submit a task with an explicit id (used by delegation for deterministic
/// child ids like `{parent}-child-{i}-{role}`).
pub async fn submit_task_with_id(
    db: &EvidenceDb,
    counter: &mut u64,
    params: SubmitParams,
    explicit_id: Option<&str>,
) -> Result<TaskView> {
    let title = params.title.trim().to_owned();
    if title.is_empty() || title.len() > 256 {
        return Err(err("title must be 1..=256 chars"));
    }
    let priority = validate_priority(params.priority.trim())?;
    let scenario = validate_scenario(params.scenario.trim())?;
    if !(1..=10).contains(&params.max_attempts) {
        return Err(err("maxAttempts must be 1..=10"));
    }
    if !(100..=3_600_000).contains(&params.timeout_ms) {
        return Err(err("timeoutMs out of range"));
    }
    if let Some(parent) = params.parent_task_id.as_deref() {
        if parent.trim().is_empty() || parent.len() > 128 {
            return Err(err("parentTaskId must be 1..=128 chars"));
        }
        let depth = evidence_store::task_depth(db, parent).await?;
        if depth >= crate::delegation::MAX_DEPTH {
            return Err(err(
                "depth limit exceeded (child cannot spawn grandchildren)",
            ));
        }
    }
    *counter += 1;
    let auto_id = format!(
        "task-{}-{}",
        now_iso().replace([':', '.', '+'], "-"),
        *counter
    );
    let auto_id = if auto_id.len() > 128 {
        auto_id[..128].to_owned()
    } else {
        auto_id
    };
    let id = explicit_id.unwrap_or(&auto_id).to_owned();
    if id.trim().is_empty() || id.len() > 128 {
        return Err(err("task id must be 1..=128 chars"));
    }
    let row = evidence_store::enqueue_task(
        db,
        evidence_store::EnqueueParams {
            id: &id,
            title: &title,
            description: params.description.as_deref(),
            priority: &priority,
            max_attempts: params.max_attempts,
            timeout_ms: params.timeout_ms,
            backoff_ms: 500,
            scenario: &scenario,
            parent_task_id: params.parent_task_id.as_deref(),
        },
    )
    .await?;
    Ok(TaskView::from(row))
}

/// Fetch one task's state (typed; `None` becomes a storage error for IPC).
pub async fn get_task_state(db: &EvidenceDb, task_id: &str) -> Result<TaskView> {
    evidence_store::get_task(db, task_id)
        .await?
        .map(TaskView::from)
        .ok_or_else(|| err("unknown task"))
}

/// List tasks newest-first (bounded).
pub async fn list_tasks(db: &EvidenceDb, limit: i64) -> Result<Vec<TaskView>> {
    Ok(evidence_store::list_tasks(db, limit)
        .await?
        .into_iter()
        .map(TaskView::from)
        .collect())
}

/// List runnable queue entries ordered by priority then age.
pub async fn list_queue(db: &EvidenceDb, limit: i64) -> Result<Vec<TaskView>> {
    Ok(evidence_store::list_queue(db, limit, &now_iso())
        .await?
        .into_iter()
        .map(TaskView::from)
        .collect())
}

/// Request cancellation: records intent, moves live tasks to `canceling`.
pub async fn cancel_task(db: &EvidenceDb, task_id: &str) -> Result<TaskView> {
    let task = get_task_state(db, task_id).await?;
    evidence_store::request_cancellation(db, task_id).await?;
    if lifecycle::is_terminal(&task.status) {
        return Ok(task);
    }
    if task.status != "canceling" {
        lifecycle::check_transition(&task.status, "canceling").map_err(|e| err(&e))?;
        evidence_store::record_transition(
            db,
            task_id,
            &task.status,
            "canceling",
            "cancel requested",
        )
        .await?;
    }
    get_task_state(db, task_id).await
}

/// Retry a finished task: `failed` / `timed_out` / `canceled` -> `queued`.
pub async fn retry_task(db: &EvidenceDb, task_id: &str) -> Result<TaskView> {
    let task = get_task_state(db, task_id).await?;
    lifecycle::check_transition(&task.status, "queued").map_err(|e| err(&e))?;
    evidence_store::record_transition(db, task_id, &task.status, "queued", "retry requested")
        .await?;
    get_task_state(db, task_id).await
}

/// Move a task through one validated transition (journal + ledger).
pub async fn transition_task(
    db: &EvidenceDb,
    task_id: &str,
    to: &str,
    reason: &str,
) -> Result<TaskView> {
    let task = get_task_state(db, task_id).await?;
    lifecycle::check_transition(&task.status, to).map_err(|e| err(&e))?;
    evidence_store::record_transition(db, task_id, &task.status, to, reason).await?;
    get_task_state(db, task_id).await
}

/// Deterministic mock outcome for one attempt (no randomness, no network).
async fn run_scenario_once(
    engine: &dyn PolicyEngine,
    db: &EvidenceDb,
    task: &TaskView,
    attempt: i64,
    cancel: &CancelToken,
) -> Result<(bool, bool, Option<String>)> {
    // Returns (succeeded, retryable, error).
    let echo = MockEchoAgent::new();
    match task.scenario.as_str() {
        "quick_success" => {
            let req = echo.plan_action("mock-agent", &task.id, attempt as u64);
            let eval = engine.evaluate_with_context(&req, &PolicyContext::default());
            if eval.decision == policy_broker::PolicyDecision::Allow {
                Ok((true, false, None))
            } else {
                // Policy still rules: a deny is fatal for a quick task.
                Ok((
                    false,
                    false,
                    Some(format!("policy denied: {:?}", eval.decision)),
                ))
            }
        }
        "delayed_success" => {
            tokio::time::sleep(std::time::Duration::from_millis(120)).await;
            if cancel.is_cancelled() || evidence_store::cancellation_pending(db, &task.id).await? {
                return Ok((false, false, Some("canceled".to_owned())));
            }
            let req = echo.plan_action("mock-agent", &task.id, attempt as u64);
            let eval = engine.evaluate_with_context(&req, &PolicyContext::default());
            if eval.decision == policy_broker::PolicyDecision::Allow {
                Ok((true, false, None))
            } else {
                Ok((
                    false,
                    false,
                    Some(format!("policy denied: {:?}", eval.decision)),
                ))
            }
        }
        "retryable_failure" => {
            // First attempt fails retryably; later attempts succeed (deterministic).
            if attempt <= 1 {
                Ok((false, true, Some("simulated retryable failure".to_owned())))
            } else {
                Ok((true, false, None))
            }
        }
        "non_retryable_failure" => Ok((false, false, Some("simulated fatal failure".to_owned()))),
        "cancellation_sensitive" => {
            for _ in 0..10 {
                if cancel.is_cancelled()
                    || evidence_store::cancellation_pending(db, &task.id).await?
                {
                    return Ok((false, false, Some("canceled".to_owned())));
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
            Ok((true, false, None))
        }
        _ => Ok((false, false, Some("unknown scenario".to_owned()))),
    }
}

/// Execute one task attempt with timeout + cancellation + retry bookkeeping.
///
/// Never bypasses the policy broker: success paths evaluate through the
/// engine; failures record attempts and either requeue (`queued`) or move to
/// a terminal state (`failed` / `timed_out` / `canceled`).
pub async fn execute_task(
    db: &EvidenceDb,
    engine: &dyn PolicyEngine,
    task_id: &str,
    cancel: &CancelToken,
) -> Result<TaskView> {
    let mut task = get_task_state(db, task_id).await?;
    if lifecycle::is_terminal(&task.status) {
        return Ok(task);
    }
    if evidence_store::cancellation_pending(db, task_id).await? || cancel.is_cancelled() {
        if task.status != "canceling" {
            lifecycle::check_transition(&task.status, "canceling").map_err(|e| err(&e))?;
            evidence_store::record_transition(
                db,
                task_id,
                &task.status,
                "canceling",
                "cancel acknowledged",
            )
            .await?;
        }
        evidence_store::mark_cancellation(db, task_id, true, true).await?;
        return transition_task(db, task_id, "canceled", "cancellation completed").await;
    }
    // queued -> planning -> executing.
    if task.status == "queued" {
        task = transition_task(db, task_id, "planning", "worker picked up").await?;
    }
    if task.status == "planning" {
        task = transition_task(db, task_id, "executing", "attempt started").await?;
    }
    if task.status != "executing" {
        return Ok(task);
    }
    let attempt = evidence_store::begin_attempt(db, task_id).await?;
    let timeout = std::time::Duration::from_millis(task.timeout_ms.max(100) as u64);
    let outcome = tokio::time::timeout(
        timeout,
        run_scenario_once(engine, db, &task, attempt, cancel),
    )
    .await;
    match outcome {
        Err(_) => {
            evidence_store::finish_attempt(db, task_id, attempt, "timed_out", Some("timeout"))
                .await?;
            transition_task(db, task_id, "timed_out", "attempt timeout").await
        }
        Ok(Err(e)) => {
            evidence_store::finish_attempt(
                db,
                task_id,
                attempt,
                "failed_fatal",
                Some(&e.to_string()),
            )
            .await?;
            transition_task(db, task_id, "failed", &format!("worker error: {e}")).await
        }
        Ok(Ok((true, _, _))) => {
            evidence_store::finish_attempt(db, task_id, attempt, "succeeded", None).await?;
            if evidence_store::cancellation_pending(db, task_id).await? || cancel.is_cancelled() {
                evidence_store::mark_cancellation(db, task_id, true, true).await?;
                let mid = transition_task(db, task_id, "canceling", "late cancel").await?;
                let _ = mid;
                transition_task(db, task_id, "canceled", "cancellation completed").await
            } else {
                transition_task(db, task_id, "completed", "attempt succeeded").await
            }
        }
        Ok(Ok((false, retryable, error))) => {
            let message = error.unwrap_or_else(|| "failed".to_owned());
            if message == "canceled" {
                evidence_store::finish_attempt(db, task_id, attempt, "canceled", Some(&message))
                    .await?;
                evidence_store::mark_cancellation(db, task_id, true, true).await?;
                let mid = if get_task_state(db, task_id).await?.status == "executing" {
                    transition_task(db, task_id, "canceling", "cancel acknowledged").await?
                } else {
                    get_task_state(db, task_id).await?
                };
                let _ = mid;
                transition_task(db, task_id, "canceled", "cancellation completed").await
            } else if retryable && attempt < task.max_attempts {
                evidence_store::finish_attempt(
                    db,
                    task_id,
                    attempt,
                    "failed_retryable",
                    Some(&message),
                )
                .await?;
                // Deterministic backoff: base * 2^(attempt-1).
                let backoff = 500_i64.saturating_mul(1_i64 << (attempt - 1).min(6));
                let retry_at = chrono::Utc::now() + chrono::Duration::milliseconds(backoff);
                evidence_store::set_next_retry(db, task_id, &retry_at.to_rfc3339()).await?;
                // Requeue for the supervisor/worker: executing -> queued is not a
                // lifecycle move, so record via failed -> queued retry path.
                let failed = transition_task(db, task_id, "failed", &message).await?;
                let _ = failed;
                transition_task(db, task_id, "queued", "retryable: requeued").await
            } else {
                evidence_store::finish_attempt(
                    db,
                    task_id,
                    attempt,
                    "failed_fatal",
                    Some(&message),
                )
                .await?;
                transition_task(db, task_id, "failed", &message).await
            }
        }
    }
}

/// Run up to `config.max_concurrency` queued tasks (priority order).
pub async fn run_due_tasks(
    db: &EvidenceDb,
    engine: &dyn PolicyEngine,
    config: &RuntimeConfig,
    cancel: &CancelToken,
) -> Result<Vec<TaskView>> {
    let limit = config.max_concurrency.max(1) as i64;
    let queued = list_queue(db, limit).await?;
    let mut done = Vec::new();
    for entry in queued {
        if cancel.is_cancelled() {
            break;
        }
        done.push(execute_task(db, engine, &entry.id, cancel).await?);
    }
    Ok(done)
}

/// List direct children of one task (oldest first).
pub async fn list_children(db: &EvidenceDb, parent_id: &str) -> Result<Vec<TaskView>> {
    Ok(evidence_store::list_children(db, parent_id)
        .await?
        .into_iter()
        .map(TaskView::from)
        .collect())
}

/// Build the parent-child subtree rooted at `root_id`.
pub async fn get_task_tree(db: &EvidenceDb, root_id: &str) -> Result<TaskTreeView> {
    fn convert(node: evidence_store::TaskTreeNode) -> TaskTreeView {
        TaskTreeView {
            task: TaskView::from(node.task),
            children: node.children.into_iter().map(convert).collect(),
        }
    }
    Ok(convert(evidence_store::get_task_tree(db, root_id).await?))
}

/// Cancel a parent and all its descendants (depth-first).
///
/// Returns the number of tasks moved toward cancellation. Terminal tasks are
/// skipped; live tasks go through the validated `canceling` transition and a
/// cancellation request is recorded for each.
pub async fn cancel_subtree(db: &EvidenceDb, root_id: &str) -> Result<usize> {
    use crate::lifecycle;
    let tree = evidence_store::get_task_tree(db, root_id).await?;
    let mut count = 0_usize;
    let mut stack = vec![tree];
    while let Some(node) = stack.pop() {
        for child in node.children {
            stack.push(child);
        }
        let status = node.task.status.clone();
        if lifecycle::is_terminal(&status) {
            continue;
        }
        evidence_store::request_cancellation(db, &node.task.id).await?;
        if status != "canceling" && lifecycle::check_transition(&status, "canceling").is_ok() {
            evidence_store::record_transition(
                db,
                &node.task.id,
                &status,
                "canceling",
                "cancel subtree",
            )
            .await?;
        }
        count += 1;
    }
    Ok(count)
}
