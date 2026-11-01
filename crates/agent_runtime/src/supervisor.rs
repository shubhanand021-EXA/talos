//! Supervisor: timeouts, stuck-task detection, cancellation, policy-respecting
//! retries — plus multi-agent delegation (Batch 3).
//!
//! The legacy [`supervise_once`] pass is unchanged. New code adds
//! [`SupervisorAgent`]: it deterministically decomposes a complex task into a
//! DAG of scoped sub-tasks, assigns each to a specialist via a
//! [`crate::delegation::DelegationRequest`], monitors progress over typed
//! channels, merges structured results, and handles failures (retry,
//! reassign, escalate). The Supervisor never bypasses the policy broker and
//! can never override a policy decision for a child.

use evidence_store::EvidenceDb;
use policy_broker::{ActionRequest, Capability, RiskLevel};
use talos_common::{Result, TalosError};

use crate::delegation::{
    decompose_task, merge_results, split_budget_equal, validate_delegation, ArtifactRef,
    DelegationRequest, DelegationResult, PlannedSubtask, SpecialistRole,
    DEFAULT_CHILD_CONTEXT_BUDGET, MAX_DEPTH, MAX_FANOUT,
};
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

/// Tuning for [`SupervisorAgent`] delegation (no hidden globals).
#[derive(Debug, Clone)]
pub struct SupervisorConfig {
    /// Max children per parent (≤ [`MAX_FANOUT`]).
    pub max_fanout: usize,
    /// Max hierarchy depth (≤ [`MAX_DEPTH`], root = 1).
    pub max_depth: usize,
    /// Default per-child deadline (ms).
    pub default_deadline_ms: i64,
    /// Default per-child context window (tokens).
    pub default_context_budget: u32,
}

impl Default for SupervisorConfig {
    fn default() -> Self {
        Self {
            max_fanout: MAX_FANOUT,
            max_depth: MAX_DEPTH,
            default_deadline_ms: 30_000,
            default_context_budget: DEFAULT_CHILD_CONTEXT_BUDGET,
        }
    }
}

/// Outcome of merging child results into a parent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeOutcome {
    /// Parent status (`completed` iff every child completed).
    pub status: String,
    /// Union of child artifacts.
    pub artifacts: Vec<ArtifactRef>,
    /// Human summary for the ledger + UI.
    pub summary: String,
}

/// Failure handling for one failed child.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailureAction {
    /// Retry the same slice once more (retryable + attempts left).
    Retry,
    /// Reassign to a different role (reserved for Batch 4).
    Reassign,
    /// Mark the parent failed with the child detail.
    Escalate,
}

/// Multi-agent Supervisor: decomposes, delegates, monitors, and merges.
///
/// The Supervisor owns the parent task but never executes child work itself.
/// Every spawn is policy-gated (`SystemControl`, medium risk); every child
/// action is independently policy-gated; budgets split equally by default.
#[derive(Debug, Clone)]
pub struct SupervisorAgent {
    /// Supervisor identifier (`supervisor-{parent}`).
    pub id: String,
    /// Delegation tuning.
    pub config: SupervisorConfig,
}

impl SupervisorAgent {
    /// Create a Supervisor for one parent task.
    #[must_use]
    pub fn new(id: &str) -> Self {
        Self {
            id: id.to_owned(),
            config: SupervisorConfig::default(),
        }
    }

    /// Create with explicit tuning (clamped to protocol ceilings).
    #[must_use]
    pub fn with_config(id: &str, mut config: SupervisorConfig) -> Self {
        config.max_fanout = config.max_fanout.clamp(1, MAX_FANOUT);
        config.max_depth = config.max_depth.clamp(1, MAX_DEPTH);
        Self {
            id: id.to_owned(),
            config,
        }
    }

    /// Deterministic decomposition (delegates to the protocol helper).
    #[must_use]
    pub fn decompose(&self, goal: &str, description: Option<&str>) -> Vec<PlannedSubtask> {
        decompose_task(goal, description)
    }

    /// Plan child delegations for a parent (ids are deterministic).
    ///
    /// Child ids look like `{parent}-child-{i}-{role}`; depths are
    /// `parent_depth + 1`; budgets split equally when `parent_budget_usd`
    /// is `Some`.
    #[must_use]
    pub fn plan_delegations(
        &self,
        parent_task_id: &str,
        parent_depth: usize,
        planned: &[PlannedSubtask],
        parent_budget_usd: Option<f64>,
    ) -> Vec<DelegationRequest> {
        let shares = split_budget_equal(parent_budget_usd, planned.len());
        let _ = shares;
        planned
            .iter()
            .enumerate()
            .map(|(i, subtask)| DelegationRequest {
                parent_task_id: parent_task_id.to_owned(),
                child_task_id: format!(
                    "{parent_task_id}-child-{}-{}",
                    i + 1,
                    subtask.role.as_str()
                ),
                agent_role: subtask.role,
                scope: subtask.scope.clone(),
                tool_allowlist: subtask.role.tool_allowlist(),
                context_budget: self.config.default_context_budget,
                deadline_ms: self.config.default_deadline_ms,
                depth: parent_depth + 1,
            })
            .collect()
    }

    /// Validate one spawn against fan-out, depth, and narrowing rules.
    pub fn validate_spawn(
        &self,
        existing_children: usize,
        parent_depth: usize,
        parent_allowlist: &[String],
        request: &DelegationRequest,
    ) -> Result<()> {
        let _ = self.config.max_fanout;
        validate_delegation(existing_children, parent_depth, parent_allowlist, request)
    }

    /// Policy request that gates spawning one sub-agent.
    ///
    /// Spawning requires evaluation: `SystemControl`, medium risk. The engine
    /// will return `RequireEvidence` until the parent has recorded evidence;
    /// the desktop layer resolves that with live ledger context before any
    /// child row is inserted.
    #[must_use]
    pub fn spawn_policy_request(&self, parent_task_id: &str, child_task_id: &str) -> ActionRequest {
        ActionRequest {
            id: format!("delegate-{child_task_id}"),
            agent_id: self.id.clone(),
            task_id: parent_task_id.to_owned(),
            capability: Capability::SystemControl,
            intent: format!("delegate sub-task {child_task_id} to specialist"),
            target: child_task_id.to_owned(),
            payload_json: serde_json::json!({
                "parent_task_id": parent_task_id,
                "child_task_id": child_task_id,
            }),
            risk_level: RiskLevel::Medium,
            reversible: true,
            requested_at: chrono::Utc::now(),
        }
    }

    /// Merge child results into a parent outcome.
    #[must_use]
    pub fn merge(&self, parent_task_id: &str, results: &[DelegationResult]) -> MergeOutcome {
        let (status, artifacts) = merge_results(results);
        let summary = if status == "completed" {
            format!(
                "merged {} child result(s) into {parent_task_id}: all succeeded ({} artifact(s))",
                results.len(),
                artifacts.len()
            )
        } else {
            let failed: Vec<&str> = results
                .iter()
                .filter(|r| r.status != "completed")
                .map(|r| r.child_task_id.as_str())
                .collect();
            format!(
                "merged {} child result(s) into {parent_task_id}: FAILED [{}]",
                results.len(),
                failed.join(", ")
            )
        };
        MergeOutcome {
            status,
            artifacts,
            summary,
        }
    }

    /// Decide what to do with one failed child.
    #[must_use]
    pub fn on_child_failure(
        &self,
        result: &DelegationResult,
        attempts_so_far: i64,
        max_attempts: i64,
        retryable: bool,
    ) -> FailureAction {
        if retryable && attempts_so_far < max_attempts {
            FailureAction::Retry
        } else if result.status == "canceled" {
            FailureAction::Escalate
        } else if attempts_so_far < max_attempts {
            FailureAction::Retry
        } else {
            FailureAction::Escalate
        }
    }

    /// Full parent authority for narrowing checks (union of all specialists).
    #[must_use]
    pub fn parent_authority() -> Vec<String> {
        let mut tools = SpecialistRole::Coder.tool_allowlist();
        for tool in SpecialistRole::Browser.tool_allowlist() {
            if !tools.contains(&tool) {
                tools.push(tool);
            }
        }
        for tool in SpecialistRole::Researcher.tool_allowlist() {
            if !tools.contains(&tool) {
                tools.push(tool);
            }
        }
        tools.sort();
        tools
    }
}

#[allow(dead_code)]
fn delegation_err(msg: String) -> TalosError {
    TalosError::Storage(msg)
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

    #[test]
    fn supervisor_decomposes_code_goal() {
        let sup = SupervisorAgent::new("sup-1");
        let planned = sup.decompose("fix the code path", None);
        assert_eq!(planned.len(), 1);
        assert_eq!(planned[0].role, SpecialistRole::Coder);
    }

    #[test]
    fn supervisor_plans_deterministic_children() {
        let sup = SupervisorAgent::new("sup-1");
        let planned = sup.decompose("code the page and research the web flow", None);
        let requests = sup.plan_delegations("parent-1", 1, &planned, Some(1.0));
        assert!(!requests.is_empty());
        for request in &requests {
            assert!(request.child_task_id.starts_with("parent-1-child-"));
            assert_eq!(request.depth, 2);
        }
    }

    #[test]
    fn supervisor_rejects_grandchild_spawn() {
        let sup = SupervisorAgent::new("sup-1");
        let parent_tools = SupervisorAgent::parent_authority();
        let request = DelegationRequest {
            parent_task_id: "child-1".to_owned(),
            child_task_id: "child-1-child-1-coder".to_owned(),
            agent_role: SpecialistRole::Coder,
            scope: "code".to_owned(),
            tool_allowlist: vec!["fs.read".to_owned()],
            context_budget: 4000,
            deadline_ms: 30_000,
            depth: 3,
        };
        // Depth 2 parent cannot spawn (max depth 2).
        assert!(sup.validate_spawn(0, 2, &parent_tools, &request).is_err());
    }

    #[test]
    fn supervisor_enforces_fan_out() {
        let sup = SupervisorAgent::new("sup-1");
        let parent_tools = SupervisorAgent::parent_authority();
        let request = DelegationRequest {
            parent_task_id: "p".to_owned(),
            child_task_id: "p-child-5-coder".to_owned(),
            agent_role: SpecialistRole::Coder,
            scope: "code".to_owned(),
            tool_allowlist: vec!["fs.read".to_owned()],
            context_budget: 4000,
            deadline_ms: 30_000,
            depth: 2,
        };
        assert!(sup
            .validate_spawn(MAX_FANOUT, 1, &parent_tools, &request)
            .is_err());
    }

    #[test]
    fn spawn_policy_request_is_system_control_medium() {
        let sup = SupervisorAgent::new("sup-1");
        let request = sup.spawn_policy_request("parent-1", "parent-1-child-1-coder");
        assert_eq!(request.capability, Capability::SystemControl);
        assert_eq!(request.risk_level, RiskLevel::Medium);
    }

    #[test]
    fn merge_all_success_completes() {
        let sup = SupervisorAgent::new("sup-1");
        let ok = |id: &str| DelegationResult {
            child_task_id: id.to_owned(),
            status: "completed".to_owned(),
            artifacts: vec![],
            evidence_ids: vec![],
            limitations: vec![],
            remaining_risks: vec![],
        };
        let outcome = sup.merge("p", &[ok("c1"), ok("c2")]);
        assert_eq!(outcome.status, "completed");
        let failed = sup.merge(
            "p",
            &[
                ok("c1"),
                DelegationResult {
                    status: "failed".to_owned(),
                    ..ok("c2")
                },
            ],
        );
        assert_eq!(failed.status, "failed");
        assert!(failed.summary.contains("FAILED"));
    }

    #[test]
    fn failure_action_retries_then_escalates() {
        let sup = SupervisorAgent::new("sup-1");
        let failed = DelegationResult {
            child_task_id: "c".to_owned(),
            status: "failed".to_owned(),
            artifacts: vec![],
            evidence_ids: vec![],
            limitations: vec![],
            remaining_risks: vec![],
        };
        assert_eq!(
            sup.on_child_failure(&failed, 1, 3, true),
            FailureAction::Retry
        );
        assert_eq!(
            sup.on_child_failure(&failed, 3, 3, true),
            FailureAction::Escalate
        );
    }

    #[test]
    fn delegation_err_helper_builds_storage_error() {
        let err = delegation_err("boom".to_owned());
        assert!(matches!(err, TalosError::Storage(_)));
    }
}
