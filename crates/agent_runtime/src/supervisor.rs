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
    DEFAULT_CHILD_CONTEXT_BUDGET, MAX_DEPTH, MAX_DEPTH_LIMIT, MAX_FANOUT, MAX_FANOUT_LIMIT,
};
use crate::planner::{
    keyword_fallback_dag, parse_plan_json, validated_to_dag, PlanningConfig, PlanningMethod,
    PlanningPromptInput, PlanningPromptTemplate, PlannedDag, ValidatedPlan,
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

/// Tuning for [`SupervisorAgent`] delegation.
///
/// Hard ceilings: fan-out 1..=16, depth 1..=4 (Perplexity §2.6). Defaults
/// are `MAX_FANOUT` (4) / `MAX_DEPTH` (2). Validation always reads these
/// fields — never the module constants.
#[derive(Debug, Clone)]
pub struct SupervisorConfig {
    /// Max children per parent (1..=16).
    pub max_fanout: usize,
    /// Max hierarchy depth (1..=4, root = 1).
    pub max_depth: usize,
    /// Default per-child deadline (ms).
    pub default_deadline_ms: i64,
    /// Default per-child context window (tokens).
    pub default_context_budget: u32,
}

impl SupervisorConfig {
    /// Check the tuning ranges (fan-out 1..=16, depth 1..=4).
    pub fn validate(&self) -> Result<()> {
        if !(1..=MAX_FANOUT_LIMIT).contains(&self.max_fanout) {
            return Err(TalosError::Storage(format!(
                "max_fanout must be 1..={} (got {})",
                MAX_FANOUT_LIMIT, self.max_fanout
            )));
        }
        if !(1..=MAX_DEPTH_LIMIT).contains(&self.max_depth) {
            return Err(TalosError::Storage(format!(
                "max_depth must be 1..={} (got {})",
                MAX_DEPTH_LIMIT, self.max_depth
            )));
        }
        Ok(())
    }
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
    /// Parent fidelity: the minimum of all children (a parent is only as
    /// verified as its least-verified child). Empty merges report `mock`.
    pub fidelity: talos_common::ExecutionFidelity,
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
///
/// Planning is LLM-driven with deterministic gates: [`SupervisorAgent::plan_with_llm`]
/// sends the goal to a model, parses the strict DAG schema, validates it via
/// [`crate::planner::validate_plan`], and falls back to keyword decomposition
/// (labeled [`PlanningMethod::Keyword`]) on any failure. LLM output can never
/// bypass policy: every step's tools still pass through the policy broker at
/// execution time.
#[derive(Debug, Clone)]
pub struct SupervisorAgent {
    /// Supervisor identifier (`supervisor-{parent}`).
    pub id: String,
    /// Delegation tuning.
    pub config: SupervisorConfig,
    /// Planning settings (model, timeout, fallback behavior).
    pub planning: PlanningConfig,
}

impl SupervisorAgent {
    /// Create a Supervisor for one parent task.
    #[must_use]
    pub fn new(id: &str) -> Self {
        Self {
            id: id.to_owned(),
            config: SupervisorConfig::default(),
            planning: PlanningConfig::default(),
        }
    }

    /// Create with explicit tuning (clamped to the hard ceilings 1..=16 /
    /// 1..=4). Use [`SupervisorConfig::validate`] to reject instead.
    #[must_use]
    pub fn with_config(id: &str, mut config: SupervisorConfig) -> Self {
        config.max_fanout = config.max_fanout.clamp(1, MAX_FANOUT_LIMIT);
        config.max_depth = config.max_depth.clamp(1, MAX_DEPTH_LIMIT);
        Self {
            id: id.to_owned(),
            config,
            planning: PlanningConfig::default(),
        }
    }

    /// Create with explicit planning settings.
    #[must_use]
    pub fn with_planning(mut self, planning: PlanningConfig) -> Self {
        self.planning = planning;
        self
    }

    /// Deterministic decomposition (delegates to the protocol helper).
    #[must_use]
    pub fn decompose(&self, goal: &str, description: Option<&str>) -> Vec<PlannedSubtask> {
        decompose_task(goal, description)
    }

    /// LLM-driven DAG planning with deterministic validation gates.
    ///
    /// Sends the goal to the model via `client`, parses the strict JSON
    /// schema, and validates it deterministically ([`crate::planner::validate_plan`]).
    /// On a parse failure the correction prompt is retried once; a second
    /// failure, a timeout (> `planning.timeout_ms`, default 30s), a missing
    /// client (no LLM key configured), or a validation rejection all fall
    /// back to keyword decomposition with a recorded reason. The outcome
    /// always labels its [`PlanningMethod`], and carries request/response
    /// hashes plus token estimates for evidence.
    pub async fn plan_with_llm(
        &self,
        task_id: &str,
        goal: &str,
        description: Option<&str>,
        available_tools: &[String],
        task_budget_usd: Option<f64>,
        client: Option<&dyn PlanningLlmClient>,
    ) -> PlannedDag {
        let template = PlanningPromptTemplate::new();
        let Some(client) = client else {
            return keyword_fallback_dag(
                goal,
                description,
                Some("no provider key configured; using deterministic keyword path".to_owned()),
            );
        };
        let prompt = template.render(&PlanningPromptInput {
            goal,
            description,
            available_tools,
            task_budget_usd,
            max_depth: self.config.max_depth,
            max_fanout: self.config.max_fanout,
        });
        let (prompt_tokens, _) =
            context_assembler::estimate_tokens(&self.planning.model, &prompt);
        let request_hash = crate::planner::hash_planning_payload(&serde_json::json!({
            "task_id": task_id,
            "template_version": crate::planner::PLANNING_PROMPT_VERSION,
            "model": self.planning.model,
            "prompt_tokens": prompt_tokens,
            "prompt": prompt,
        }));
        let timeout = std::time::Duration::from_millis(self.planning.timeout_ms);
        // First attempt.
        let first = tokio::time::timeout(timeout, client.complete(prompt.clone())).await;
        let raw = match first {
            Err(_) => {
                return keyword_fallback_dag(
                    goal,
                    description,
                    Some(format!(
                        "planning timed out after {}ms; using keyword path",
                        self.planning.timeout_ms
                    )),
                );
            }
            Ok(Err(err)) => {
                return keyword_fallback_dag(
                    goal,
                    description,
                    Some(format!("planning call failed ({err}); using keyword path")),
                );
            }
            Ok(Ok(text)) => text,
        };
        let validation_ctx = crate::planner::ValidationContext {
            max_depth: self.config.max_depth,
            max_fanout: self.config.max_fanout,
            allowed_tools: available_tools.to_vec(),
            token_budget: task_budget_usd.map(token_budget_from_usd),
        };
        let attempt = |text: &str| -> std::result::Result<ValidatedPlan, crate::planner::PlanRejection> {
            let parsed = parse_plan_json(text)?;
            crate::planner::validate_plan(&parsed, &validation_ctx)
        };
        let validated = match attempt(&raw) {
            Ok(validated) => validated,
            Err(first_err) => {
                // Single retry with a correction prompt.
                let correction = template.correction_prompt(&raw, &first_err.to_string());
                let second =
                    tokio::time::timeout(timeout, client.complete(correction)).await;
                let text = match second {
                    Err(_) => {
                        return keyword_fallback_dag(
                            goal,
                            description,
                            Some(format!(
                                "planning retry timed out after {}ms; using keyword path",
                                self.planning.timeout_ms
                            )),
                        );
                    }
                    Ok(Err(err)) => {
                        return keyword_fallback_dag(
                            goal,
                            description,
                            Some(format!(
                                "planning retry failed ({err}); using keyword path"
                            )),
                        );
                    }
                    Ok(Ok(text)) => text,
                };
                match attempt(&text) {
                    Ok(validated) => validated,
                    Err(second_err) => {
                        return keyword_fallback_dag(
                            goal,
                            description,
                            Some(format!(
                                "LLM plan invalid twice ({first_err}; retry: {second_err}); using keyword path"
                            )),
                        );
                    }
                }
            }
        };
        let (completion_tokens, _) =
            context_assembler::estimate_tokens(&self.planning.model, &raw);
        let response_hash = crate::planner::hash_planning_payload(&serde_json::json!({
            "task_id": task_id,
            "model": self.planning.model,
            "completion_tokens": completion_tokens,
            "response": raw,
        }));
        validated_to_dag(
            &validated,
            Some(request_hash),
            Some(response_hash),
        )
    }

    /// Plan child delegations for a validated DAG (topological order).
    ///
    /// Child ids look like `{parent}-step-{step_id}-{role}`; steps carry
    /// their `depends_on` edges so the caller can enforce execution order
    /// (see [`crate::planner::execution_levels`]).
    #[must_use]
    pub fn plan_dag_delegations(
        &self,
        parent_task_id: &str,
        parent_depth: usize,
        dag: &PlannedDag,
        parent_budget_usd: Option<f64>,
    ) -> Vec<DelegationRequest> {
        let shares = split_budget_equal(parent_budget_usd, dag.steps.len());
        dag.steps
            .iter()
            .enumerate()
            .map(|(i, step)| {
                let allowlist = if step.tools_needed.is_empty() {
                    step.role.tool_allowlist()
                } else {
                    step.tools_needed.clone()
                };
                DelegationRequest {
                    parent_task_id: parent_task_id.to_owned(),
                    child_task_id: format!(
                        "{parent_task_id}-step-{}-{}",
                        sanitize_step_id(&step.step_id),
                        step.role.as_str()
                    ),
                    agent_role: step.role,
                    scope: step.scope.clone(),
                    tool_allowlist: allowlist,
                    context_budget: self.config.default_context_budget,
                    deadline_ms: self.config.default_deadline_ms,
                    depth: parent_depth + 1,
                    budget_usd: shares.get(i).copied().flatten(),
                }
            })
            .collect()
    }

    /// Typed lifecycle events for one planning pass (for the ledger + UI).
    #[must_use]
    pub fn planning_events(task_id: &str, dag: &PlannedDag) -> Vec<crate::RuntimeEvent> {
        let mut events = vec![crate::RuntimeEvent::PlanGenerated {
            task_id: task_id.to_owned(),
            method: dag.method.as_str().to_owned(),
            step_count: dag.steps.len(),
        }];
        match dag.method {
            PlanningMethod::Llm => {
                let levels = dag
                    .levels
                    .iter()
                    .map(|level| {
                        level
                            .iter()
                            .map(|id| format!("{task_id}-step-{id}"))
                            .collect()
                    })
                    .collect();
                events.push(crate::RuntimeEvent::PlanValidated {
                    task_id: task_id.to_owned(),
                    levels,
                    requires_approval: dag.requires_approval,
                });
            }
            PlanningMethod::Keyword => {
                events.push(crate::RuntimeEvent::PlanFallback {
                    task_id: task_id.to_owned(),
                    reason: dag
                        .fallback_reason
                        .clone()
                        .unwrap_or_else(|| "keyword decomposition".to_owned()),
                });
            }
        }
        events
    }

    /// Plan child delegations for a parent (ids are deterministic).
    ///
    /// Child ids look like `{parent}-child-{i}-{role}`; depths are
    /// `parent_depth + 1`; each request carries its equal budget share
    /// (`None` when the parent is uncapped — children are uncapped too).
    #[must_use]
    pub fn plan_delegations(
        &self,
        parent_task_id: &str,
        parent_depth: usize,
        planned: &[PlannedSubtask],
        parent_budget_usd: Option<f64>,
    ) -> Vec<DelegationRequest> {
        let shares = split_budget_equal(parent_budget_usd, planned.len());
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
                budget_usd: shares.get(i).copied().flatten(),
            })
            .collect()
    }

    /// Validate one spawn against the supervisor's own fan-out, depth, and
    /// narrowing rules (TAL-005: config is enforced, not ignored).
    pub fn validate_spawn(
        &self,
        existing_children: usize,
        parent_depth: usize,
        parent_allowlist: &[String],
        request: &DelegationRequest,
    ) -> Result<()> {
        validate_delegation(
            existing_children,
            parent_depth,
            parent_allowlist,
            request,
            self.config.max_fanout,
            self.config.max_depth,
        )
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
    ///
    /// Parent fidelity is the minimum of all children: any `mock` child
    /// caps the parent at `mock`, any non-`verified` child caps it below
    /// `verified`. A parent is `failed` when any child failed.
    #[must_use]
    pub fn merge(&self, parent_task_id: &str, results: &[DelegationResult]) -> MergeOutcome {
        let (status, artifacts) = merge_results(results);
        let fidelity = results
            .iter()
            .map(|r| r.fidelity)
            .reduce(|a, b| a.min(b))
            .unwrap_or(talos_common::ExecutionFidelity::Mock);
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
            fidelity,
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

/// Client for one LLM planning call (implemented by the gateway adapter in
/// the desktop layer and by mocks in tests).
#[async_trait::async_trait]
pub trait PlanningLlmClient: Send + Sync {
    /// Complete the planning prompt; `Err` is a transport-level failure.
    async fn complete(&self, prompt: String) -> std::result::Result<String, String>;
}

/// Convert a USD task cap into a planning token budget.
///
/// Uses the gateway's cheapest blended rate assumption ($0.01 per 1k tokens):
/// `$1 ≈ 100k tokens`. The conversion is deterministic and documented in
/// evidence; callers that track exact model pricing should pass an explicit
/// token budget instead.
#[must_use]
pub fn token_budget_from_usd(budget_usd: f64) -> u64 {
    (budget_usd.max(0.0) * 100_000.0) as u64
}

/// Sanitize a plan step id for use inside a child task id.
fn sanitize_step_id(step_id: &str) -> String {
    let mut out: String = step_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if out.is_empty() {
        out.push_str("step");
    }
    out.truncate(48);
    out
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

    struct MockPlanningClient {
        responses: std::sync::Mutex<Vec<std::result::Result<String, String>>>,
    }

    impl MockPlanningClient {
        fn new(responses: Vec<std::result::Result<String, String>>) -> Self {
            Self {
                responses: std::sync::Mutex::new(responses),
            }
        }
    }

    #[async_trait::async_trait]
    impl crate::supervisor::PlanningLlmClient for MockPlanningClient {
        async fn complete(&self, _prompt: String) -> std::result::Result<String, String> {
            self.responses
                .lock()
                .expect("mock lock")
                .pop()
                .expect("mock response queued")
        }
    }

    fn llm_plan_json() -> String {
        serde_json::json!({
            "plan_version": "1.0",
            "steps": [
                {"step_id": "a", "role": "researcher", "goal": "gather context on the bug", "depends_on": [], "tools_needed": ["fs.read"], "risk_level": "low", "estimated_tokens": 100},
                {"step_id": "b", "role": "coder", "goal": "apply the code fix", "depends_on": ["a"], "tools_needed": ["fs.read"], "risk_level": "medium", "estimated_tokens": 200}
            ],
            "total_estimated_tokens": 300,
            "reasoning": "research then code"
        })
        .to_string()
    }

    fn planning_tools() -> Vec<String> {
        let mut tools = SupervisorAgent::parent_authority();
        tools.sort();
        tools
    }

    #[tokio::test]
    async fn llm_planning_produces_valid_dag_steps() {
        let sup = SupervisorAgent::new("sup-1");
        let client = MockPlanningClient::new(vec![Ok(llm_plan_json())]);
        let dag = sup
            .plan_with_llm(
                "parent-1",
                "fix the bug",
                None,
                &planning_tools(),
                None,
                Some(&client),
            )
            .await;
        assert_eq!(dag.method, crate::planner::PlanningMethod::Llm);
        assert_eq!(dag.steps.len(), 2);
        assert_eq!(dag.steps[0].step_id, "a");
        assert_eq!(dag.steps[1].depends_on, vec!["a"]);
        assert!(dag.request_hash.is_some());
        assert!(dag.response_hash.is_some());
        // DAG delegations carry the dependency structure in spawn order.
        let requests = sup.plan_dag_delegations("parent-1", 1, &dag, Some(1.0));
        assert_eq!(requests.len(), 2);
        assert!(requests[0].child_task_id.contains("-step-a-"));
        assert_eq!(requests[0].depth, 2);
    }

    #[tokio::test]
    async fn invalid_json_falls_back_to_keyword() {
        let sup = SupervisorAgent::new("sup-1");
        let client = MockPlanningClient::new(vec![
            Ok("still not json".to_owned()),
            Ok("not json at all".to_owned()),
        ]);
        let dag = sup
            .plan_with_llm(
                "parent-1",
                "fix the code path",
                None,
                &planning_tools(),
                None,
                Some(&client),
            )
            .await;
        assert_eq!(dag.method, crate::planner::PlanningMethod::Keyword);
        assert!(dag.fallback_reason.is_some());
        assert!(!dag.steps.is_empty());
    }

    #[tokio::test]
    async fn rejected_llm_plan_falls_back_with_reason() {
        let sup = SupervisorAgent::new("sup-1");
        // Circular plan, twice: both attempts rejected.
        let circular = serde_json::json!({
            "plan_version": "1.0",
            "steps": [
                {"step_id": "a", "role": "coder", "goal": "do a", "depends_on": ["b"], "tools_needed": ["fs.read"], "risk_level": "low", "estimated_tokens": 100},
                {"step_id": "b", "role": "coder", "goal": "do b", "depends_on": ["a"], "tools_needed": ["fs.read"], "risk_level": "low", "estimated_tokens": 100}
            ],
            "total_estimated_tokens": 200,
            "reasoning": "cycle"
        })
        .to_string();
        let client =
            MockPlanningClient::new(vec![Ok(circular.clone()), Ok(circular)]);
        let dag = sup
            .plan_with_llm(
                "parent-1",
                "fix the code path",
                None,
                &planning_tools(),
                None,
                Some(&client),
            )
            .await;
        assert_eq!(dag.method, crate::planner::PlanningMethod::Keyword);
        let reason = dag.fallback_reason.expect("reason recorded");
        assert!(reason.contains("circular") || reason.contains("dependency"));
    }

    #[tokio::test]
    async fn missing_client_falls_back_to_keyword() {
        let sup = SupervisorAgent::new("sup-1");
        let dag = sup
            .plan_with_llm(
                "parent-1",
                "fix the code path",
                None,
                &planning_tools(),
                None,
                None,
            )
            .await;
        assert_eq!(dag.method, crate::planner::PlanningMethod::Keyword);
        assert!(dag
            .fallback_reason
            .as_deref()
            .unwrap_or_default()
            .contains("no provider key"));
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
            budget_usd: None,
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
            budget_usd: None,
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
            fidelity: talos_common::ExecutionFidelity::Mock,
        };
        let outcome = sup.merge("p", &[ok("c1"), ok("c2")]);
        assert_eq!(outcome.status, "completed");
        assert_eq!(outcome.fidelity, talos_common::ExecutionFidelity::Mock);
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
    fn merge_parent_fidelity_is_minimum_of_children() {
        // Mock child caps the parent: a mock child can never yield a
        // verified parent.
        let sup = SupervisorAgent::new("sup-1");
        let child = |id: &str, fidelity| DelegationResult {
            child_task_id: id.to_owned(),
            status: "completed".to_owned(),
            artifacts: vec![],
            evidence_ids: vec![],
            limitations: vec![],
            remaining_risks: vec![],
            fidelity,
        };
        use talos_common::ExecutionFidelity as F;
        let mixed = sup.merge("p", &[child("c1", F::Verified), child("c2", F::Mock)]);
        assert_eq!(mixed.status, "completed");
        assert_eq!(mixed.fidelity, F::Mock);
        // All verified → verified parent.
        let verified = sup.merge("p", &[child("c1", F::Verified), child("c2", F::Live)]);
        assert_eq!(verified.status, "completed");
        assert_eq!(verified.fidelity, F::Live);
        let all_verified = sup.merge("p", &[child("c1", F::Verified)]);
        assert_eq!(all_verified.fidelity, F::Verified);
    }

    #[test]
    fn mock_child_cannot_yield_verified_parent() {
        // The mismatch guard fires when anyone tries to upgrade a mock
        // lineage: check the claim before trusting it.
        let sup = SupervisorAgent::new("sup-1");
        let mock_child = DelegationResult {
            child_task_id: "c".to_owned(),
            status: "completed".to_owned(),
            artifacts: vec![],
            evidence_ids: vec![],
            limitations: vec!["mock".to_owned()],
            remaining_risks: vec![],
            fidelity: talos_common::ExecutionFidelity::Mock,
        };
        let outcome = sup.merge("p", &[mock_child]);
        assert_eq!(outcome.fidelity, talos_common::ExecutionFidelity::Mock);
        assert!(talos_common::check_fidelity_claim(
            outcome.fidelity,
            talos_common::ExecutionFidelity::Verified
        )
        .is_err());
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
            fidelity: talos_common::ExecutionFidelity::Mock,
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

    fn spawn_request(child: &str, depth: usize) -> DelegationRequest {
        DelegationRequest {
            parent_task_id: "p".to_owned(),
            child_task_id: child.to_owned(),
            agent_role: SpecialistRole::Coder,
            scope: "code".to_owned(),
            tool_allowlist: vec!["fs.read".to_owned()],
            context_budget: 4000,
            deadline_ms: 30_000,
            depth,
            budget_usd: None,
        }
    }

    fn configured_supervisor(max_fanout: usize, max_depth: usize) -> SupervisorAgent {
        SupervisorAgent::with_config(
            "sup-1",
            SupervisorConfig {
                max_fanout,
                max_depth,
                ..SupervisorConfig::default()
            },
        )
    }

    #[test]
    fn delegations_carry_equal_budget_shares() {
        // TAL-004: $1.00 across N children → each request holds its share.
        let sup = SupervisorAgent::new("sup-1");
        let planned = sup.decompose("code the page and research the web flow", None);
        assert!(planned.len() >= 2);
        let requests = sup.plan_delegations("parent-1", 1, &planned, Some(1.0));
        let share = (1.0 / requests.len() as f64 * 1_000_000.0).round() / 1_000_000.0;
        for request in &requests {
            assert_eq!(request.budget_usd, Some(share));
        }
        // Uncapped parent → uncapped children.
        let uncapped = sup.plan_delegations("parent-1", 1, &planned, None);
        assert!(uncapped.iter().all(|r| r.budget_usd.is_none()));
    }

    #[test]
    fn custom_fanout_denies_third_child() {
        // TAL-005: configured max_fanout=2 is enforced (not the constant).
        let sup = configured_supervisor(2, 2);
        let parent_tools = SupervisorAgent::parent_authority();
        assert!(sup
            .validate_spawn(0, 1, &parent_tools, &spawn_request("c1", 2))
            .is_ok());
        assert!(sup
            .validate_spawn(1, 1, &parent_tools, &spawn_request("c2", 2))
            .is_ok());
        let denied = sup.validate_spawn(2, 1, &parent_tools, &spawn_request("c3", 2));
        assert!(denied.is_err());
        assert!(denied.unwrap_err().to_string().contains("fan-out"));
    }

    #[test]
    fn custom_depth_one_denies_any_child() {
        // TAL-005: configured max_depth=1 denies every spawn.
        let sup = configured_supervisor(4, 1);
        let parent_tools = SupervisorAgent::parent_authority();
        let denied = sup.validate_spawn(0, 1, &parent_tools, &spawn_request("c1", 2));
        assert!(denied.is_err());
        assert!(denied.unwrap_err().to_string().contains("depth"));
    }

    #[test]
    fn default_config_still_allows_baseline() {
        // TAL-005: defaults (fanout 4, depth 2) keep Batch 3 behavior.
        let sup = SupervisorAgent::new("sup-1");
        let parent_tools = SupervisorAgent::parent_authority();
        for existing in 0..MAX_FANOUT {
            assert!(sup
                .validate_spawn(existing, 1, &parent_tools, &spawn_request("c", 2))
                .is_ok());
        }
        assert!(sup
            .validate_spawn(MAX_FANOUT, 1, &parent_tools, &spawn_request("c", 2))
            .is_err());
    }

    #[test]
    fn config_validation_rejects_out_of_range() {
        let bad_fanout = SupervisorConfig {
            max_fanout: 0,
            ..SupervisorConfig::default()
        };
        assert!(bad_fanout.validate().is_err());
        let bad_fanout = SupervisorConfig {
            max_fanout: MAX_FANOUT_LIMIT + 1,
            ..SupervisorConfig::default()
        };
        assert!(bad_fanout.validate().is_err());
        let bad_depth = SupervisorConfig {
            max_depth: 0,
            ..SupervisorConfig::default()
        };
        assert!(bad_depth.validate().is_err());
        let bad_depth = SupervisorConfig {
            max_depth: MAX_DEPTH_LIMIT + 1,
            ..SupervisorConfig::default()
        };
        assert!(bad_depth.validate().is_err());
        assert!(SupervisorConfig::default().validate().is_ok());
        // with_config clamps to the ceilings instead of accepting garbage.
        let clamped = SupervisorAgent::with_config(
            "sup-1",
            SupervisorConfig {
                max_fanout: 100,
                max_depth: 100,
                ..SupervisorConfig::default()
            },
        );
        assert_eq!(clamped.config.max_fanout, MAX_FANOUT_LIMIT);
        assert_eq!(clamped.config.max_depth, MAX_DEPTH_LIMIT);
    }
}
