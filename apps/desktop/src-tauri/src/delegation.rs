//! Multi-agent delegation IPC: `delegate_task`, `get_task_tree`, `cancel_subtree`.
//!
//! The Supervisor decomposes a parent task, spawns scoped children (each
//! policy-gated as `SystemControl`/medium, each independently ledger-backed),
//! executes them through the durable queue, merges structured artifacts, and
//! emits `agentDelegated` / `childTaskSpawned` / `childTaskCompleted` /
//! `childTaskFailed` / `resultsMerged` for the UI and Void Neon.

use agent_runtime::{
    delegation::{DelegationResult, SpecialistRole},
    planner::{PlannedDag, PlanningConfig, PlanningMethod},
    queue as q,
    specialists::{BrowserAgent, CoderAgent, ResearchAgent, Specialist},
    supervisor::{PlanningLlmClient, SupervisorAgent, SupervisorConfig},
};
use policy_broker::{PolicyContext, PolicyEngine};
use talos_common::TalosError;

use crate::commands::IpcError;
use crate::events::{
    emit_talos_event, AGENT_DELEGATED, CHILD_BUDGET_EXCEEDED, CHILD_TASK_COMPLETED,
    CHILD_TASK_FAILED, CHILD_TASK_SPAWNED, PLAN_FALLBACK, PLAN_GENERATED, PLAN_REJECTED,
    PLAN_VALIDATED, RESULTS_MERGED, TASK_UPDATED,
};
use crate::state::AppState;

/// Supervisor tuning surfaced over IPC (camelCase JSON).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupervisorConfigView {
    /// Max children per parent (1..=16).
    pub max_fanout: usize,
    /// Max hierarchy depth (1..=4, root = 1).
    pub max_depth: usize,
    /// Default per-child deadline (ms).
    pub default_deadline_ms: i64,
    /// Default per-child context window (tokens).
    pub default_context_budget: u32,
}

impl From<&SupervisorConfig> for SupervisorConfigView {
    fn from(config: &SupervisorConfig) -> Self {
        Self {
            max_fanout: config.max_fanout,
            max_depth: config.max_depth,
            default_deadline_ms: config.default_deadline_ms,
            default_context_budget: config.default_context_budget,
        }
    }
}

/// `update_supervisor_config` — replace fan-out/depth tuning at runtime.
///
/// Validated (fan-out 1..=16, depth 1..=4) and evidence-logged; invalid
/// values are rejected without touching the live config.
#[tauri::command]
pub async fn update_supervisor_config(
    state: tauri::State<'_, AppState>,
    max_fanout: usize,
    max_depth: usize,
) -> Result<SupervisorConfigView, IpcError> {
    update_supervisor_config_inner(&state, max_fanout, max_depth).await
}

/// Inner config update (shared by the command and tests: no `AppHandle`).
pub async fn update_supervisor_config_inner(
    state: &AppState,
    max_fanout: usize,
    max_depth: usize,
) -> Result<SupervisorConfigView, IpcError> {
    let current = state.supervisor_config.lock().await.clone();
    let next = SupervisorConfig {
        max_fanout,
        max_depth,
        default_deadline_ms: current.default_deadline_ms,
        default_context_budget: current.default_context_budget,
    };
    next.validate().map_err(IpcError::from)?;
    *state.supervisor_config.lock().await = next.clone();
    let _ = evidence_store::append_event(
        &state.db,
        evidence_store::NewEvent {
            id: format!("supervisor-config-{}", uuid::Uuid::new_v4()),
            kind: "supervisor.config_updated".to_owned(),
            payload_json: serde_json::json!({
                "max_fanout": next.max_fanout,
                "max_depth": next.max_depth,
            }),
        },
    )
    .await;
    Ok(SupervisorConfigView::from(&next))
}

/// Planning settings surfaced over IPC (camelCase JSON).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanningConfigView {
    /// Planning model id.
    pub model: String,
    /// Planning LLM timeout in milliseconds.
    pub timeout_ms: u64,
    /// Whether planning failures fall back to keyword decomposition.
    pub fallback_enabled: bool,
}

impl From<&PlanningConfig> for PlanningConfigView {
    fn from(config: &PlanningConfig) -> Self {
        Self {
            model: config.model.clone(),
            timeout_ms: config.timeout_ms,
            fallback_enabled: config.fallback_enabled,
        }
    }
}

/// `get_planning_config` — current planning settings.
#[tauri::command]
pub async fn get_planning_config(
    state: tauri::State<'_, AppState>,
) -> Result<PlanningConfigView, IpcError> {
    Ok(get_planning_config_inner(&state).await)
}

/// Inner planning-config read (shared by the command and tests).
pub async fn get_planning_config_inner(state: &AppState) -> PlanningConfigView {
    PlanningConfigView::from(&*state.planning_config.lock().await)
}

/// `update_planning_config` — replace model/timeout/fallback at runtime.
///
/// Validated (model `1..=128`, timeout `1000..=120000`) and evidence-logged;
/// invalid values are rejected without touching the live config.
#[tauri::command]
pub async fn update_planning_config(
    state: tauri::State<'_, AppState>,
    model: String,
    timeout_ms: u64,
    fallback_enabled: bool,
) -> Result<PlanningConfigView, IpcError> {
    update_planning_config_inner(&state, model, timeout_ms, fallback_enabled).await
}

/// Inner planning-config update (shared by the command and tests).
pub async fn update_planning_config_inner(
    state: &AppState,
    model: String,
    timeout_ms: u64,
    fallback_enabled: bool,
) -> Result<PlanningConfigView, IpcError> {
    let next = PlanningConfig {
        model,
        timeout_ms,
        fallback_enabled,
    };
    next.validate()
        .map_err(|e| IpcError::from(TalosError::Storage(e)))?;
    *state.planning_config.lock().await = next.clone();
    let _ = evidence_store::append_event(
        &state.db,
        evidence_store::NewEvent {
            id: format!("planning-config-{}", uuid::Uuid::new_v4()),
            kind: "planning.config_updated".to_owned(),
            payload_json: serde_json::json!({
                "model": next.model,
                "timeout_ms": next.timeout_ms,
                "fallback_enabled": next.fallback_enabled,
            }),
        },
    )
    .await;
    Ok(PlanningConfigView::from(&next))
}

/// Gateway-backed planning client: one LLM call through the policy-gated
/// gateway (budget-enforced, evidence-recorded). Any transport, policy, or
/// budget failure surfaces as `Err` so the Supervisor falls back to the
/// deterministic keyword path.
struct GatewayPlanningClient<'a> {
    llm: &'a tokio::sync::Mutex<llm_gateway::LlmGateway>,
    task_id: String,
    agent_id: String,
    model: String,
    budget_usd: Option<f64>,
}

#[async_trait::async_trait]
impl<'a> PlanningLlmClient for GatewayPlanningClient<'a> {
    async fn complete(&self, prompt: String) -> std::result::Result<String, String> {
        let request = llm_gateway::LlmRequest {
            model: self.model.clone(),
            messages: vec![
                llm_gateway::LlmMessage::new(
                    llm_gateway::LlmRole::System,
                    agent_runtime::planner::PLANNING_SYSTEM_PROMPT,
                ),
                llm_gateway::LlmMessage::new(llm_gateway::LlmRole::User, prompt),
            ],
            tools: None,
            temperature: Some(0.0),
            max_tokens: Some(2048),
            stop_sequences: Vec::new(),
            metadata: llm_gateway::LlmRequestMetadata {
                task_id: self.task_id.clone(),
                agent_id: self.agent_id.clone(),
                purpose: "supervisor planning".to_owned(),
            },
        };
        let gateway = self.llm.lock().await;
        gateway
            .complete(request, self.budget_usd)
            .await
            .map(|(response, _)| response.content.unwrap_or_default())
            .map_err(|err| err.to_string())
    }
}

impl std::fmt::Debug for GatewayPlanningClient<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GatewayPlanningClient")
            .field("task_id", &self.task_id)
            .field("model", &self.model)
            .finish()
    }
}

/// True when at least one key-backed provider is configured (Ollama needs no
/// key and is always reported configured, so it is excluded here).
async fn any_provider_key_configured(state: &AppState) -> bool {
    state
        .llm
        .lock()
        .await
        .provider_statuses()
        .iter()
        .any(|(id, configured)| *configured && id != "ollama")
}

/// Run LLM-driven planning for a parent: prompt → validate → evidence → events.
///
/// Returns the [`PlannedDag`] (LLM or labeled keyword fallback). The DAG
/// structure is recorded as a `plan_artifact` evidence row plus a
/// `planning.completed` ledger event; request/response hashes (never content)
/// link the gateway call. Emits `planGenerated` plus `planValidated` (LLM) or
/// `planFallback` (keyword).
async fn plan_for_parent(
    state: &AppState,
    app: Option<&tauri::AppHandle>,
    supervisor: &SupervisorAgent,
    parent_task_id: &str,
    goal_text: &str,
    description: Option<&str>,
) -> Result<PlannedDag, IpcError> {
    let available_tools: Vec<String> = {
        let registry = state.tools.lock().await;
        registry.list().iter().map(|m| m.id.clone()).collect()
    };
    let task_budget = evidence_store::get_task_budget(&state.db, parent_task_id).await?;
    // Fast path: no provider key and no local model targeted → skip the
    // doomed LLM call and fall back immediately (still labeled + evidenced).
    let client = if any_provider_key_configured(state).await
        || supervisor.planning.model.starts_with("ollama")
    {
        Some(GatewayPlanningClient {
            llm: &state.llm,
            task_id: parent_task_id.to_owned(),
            agent_id: supervisor.id.clone(),
            model: supervisor.planning.model.clone(),
            budget_usd: task_budget,
        })
    } else {
        None
    };
    let dag = supervisor
        .plan_with_llm(
            parent_task_id,
            goal_text,
            description,
            &available_tools,
            task_budget,
            client.as_ref().map(|c| c as &dyn PlanningLlmClient),
        )
        .await;
    // Record the plan artifact (never raw prompts or completions).
    let artifact_payload = match dag.method {
        PlanningMethod::Llm => {
            // Rebuild the validated plan view from the DAG for the artifact.
            serde_json::json!({
                "plan_version": "1.0",
                "planning_method": dag.method.as_str(),
                "template_version": agent_runtime::planner::PLANNING_PROMPT_VERSION,
                "requires_approval": dag.requires_approval,
                "levels": dag.levels,
                "steps": dag.steps.iter().map(|s| serde_json::json!({
                    "step_id": s.step_id,
                    "role": s.role.as_str(),
                    "goal": s.scope,
                    "depends_on": s.depends_on,
                    "tools_needed": s.tools_needed,
                    "requires_approval": s.requires_approval,
                    "estimated_tokens": s.estimated_tokens,
                })).collect::<Vec<_>>(),
            })
        }
        PlanningMethod::Keyword => serde_json::json!({
            "plan_version": "1.0",
            "planning_method": dag.method.as_str(),
            "template_version": agent_runtime::planner::PLANNING_PROMPT_VERSION,
            "requires_approval": false,
            "fallback_reason": dag.fallback_reason,
            "steps": dag.steps.iter().map(|s| serde_json::json!({
                "step_id": s.step_id,
                "role": s.role.as_str(),
                "goal": s.scope,
            })).collect::<Vec<_>>(),
        }),
    };
    let evidence_id = format!("plan-{parent_task_id}");
    if evidence_store::insert_evidence(
        &state.db,
        evidence_store::NewEvidence {
            id: evidence_id.clone(),
            task_id: Some(parent_task_id.to_owned()),
            agent_id: Some(supervisor.id.clone()),
            session_id: Some("session-local".to_owned()),
            kind: "plan_artifact".to_owned(),
            label: Some(format!("{} plan", dag.method.as_str())),
            summary: Some(format!(
                "{} planned {} step(s)",
                dag.method.as_str(),
                dag.steps.len()
            )),
            uri: None,
            mime_type: None,
            payload: artifact_payload,
        },
    )
    .await
    .is_ok()
    {
        let _ =
            evidence_store::transition_evidence_status(&state.db, &evidence_id, "verified").await;
    }
    record_delegation_event(
        state,
        "planning.completed",
        agent_runtime::planner::planning_evidence_payload(
            parent_task_id,
            dag.method,
            dag.request_hash.as_deref(),
            dag.response_hash.as_deref(),
            dag.fallback_reason.as_deref(),
            dag.requires_approval,
        ),
    )
    .await?;
    for event in SupervisorAgent::planning_events(parent_task_id, &dag) {
        let (name, payload) = match event {
            agent_runtime::RuntimeEvent::PlanGenerated {
                task_id,
                method,
                step_count,
            } => (
                PLAN_GENERATED,
                serde_json::json!({
                    "taskId": task_id,
                    "method": method,
                    "stepCount": step_count,
                    "checkedAt": talos_common::now_iso(),
                }),
            ),
            agent_runtime::RuntimeEvent::PlanValidated {
                task_id,
                levels,
                requires_approval,
            } => (
                PLAN_VALIDATED,
                serde_json::json!({
                    "taskId": task_id,
                    "levels": levels,
                    "requiresApproval": requires_approval,
                    "checkedAt": talos_common::now_iso(),
                }),
            ),
            agent_runtime::RuntimeEvent::PlanFallback { task_id, reason } => (
                PLAN_FALLBACK,
                serde_json::json!({
                    "taskId": task_id,
                    "reason": reason,
                    "checkedAt": talos_common::now_iso(),
                }),
            ),
            agent_runtime::RuntimeEvent::PlanRejected {
                task_id,
                code,
                reason,
            } => (
                PLAN_REJECTED,
                serde_json::json!({
                    "taskId": task_id,
                    "code": code,
                    "reason": reason,
                    "checkedAt": talos_common::now_iso(),
                }),
            ),
            _ => continue,
        };
        if let Some(app) = app {
            let _ = emit_talos_event(app, name, payload);
        }
    }
    Ok(dag)
}

/// One planned step for the plan viewer (camelCase JSON).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanStepView {
    /// Plan step id.
    pub step_id: String,
    /// Specialist role.
    pub role: String,
    /// Bounded step goal.
    pub goal: String,
    /// Step ids that must complete first.
    pub depends_on: Vec<String>,
    /// Tool ids the step needs.
    pub tools_needed: Vec<String>,
    /// True when this step is high-risk.
    pub requires_approval: bool,
    /// Estimated completion tokens.
    pub estimated_tokens: u32,
}

/// `preview_plan` outcome: the generated plan without executing it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanPreviewView {
    /// Parent task previewed.
    pub parent_task_id: String,
    /// Planning method (`llm` | `keyword`).
    pub method: String,
    /// Steps in topological order.
    pub steps: Vec<PlanStepView>,
    /// Parallel execution levels (step ids).
    pub levels: Vec<Vec<String>>,
    /// True when a high-risk step needs human approval.
    pub requires_approval: bool,
    /// Keyword-fallback reason, if any.
    pub fallback_reason: Option<String>,
    /// Whether the plan passed every deterministic gate.
    pub valid: bool,
}

impl PlanPreviewView {
    /// Build from a finished planning pass.
    fn from_dag(parent_task_id: &str, dag: &PlannedDag) -> Self {
        Self {
            parent_task_id: parent_task_id.to_owned(),
            method: dag.method.as_str().to_owned(),
            steps: dag
                .steps
                .iter()
                .map(|s| PlanStepView {
                    step_id: s.step_id.clone(),
                    role: s.role.as_str().to_owned(),
                    goal: s.scope.clone(),
                    depends_on: s.depends_on.clone(),
                    tools_needed: s.tools_needed.clone(),
                    requires_approval: s.requires_approval,
                    estimated_tokens: s.estimated_tokens,
                })
                .collect(),
            levels: dag.levels.clone(),
            requires_approval: dag.requires_approval,
            fallback_reason: dag.fallback_reason.clone(),
            valid: !dag.steps.is_empty(),
        }
    }
}

/// `preview_plan` — generate the DAG plan for a task without executing it.
#[tauri::command]
pub async fn preview_plan(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    parent_task_id: String,
    goal: Option<String>,
) -> Result<PlanPreviewView, IpcError> {
    preview_plan_inner(&state, Some(&app), parent_task_id, goal).await
}

/// Inner plan preview (shared by the command and tests).
pub async fn preview_plan_inner(
    state: &AppState,
    app: Option<&tauri::AppHandle>,
    parent_task_id: String,
    goal: Option<String>,
) -> Result<PlanPreviewView, IpcError> {
    if parent_task_id.trim().is_empty() || parent_task_id.len() > 128 {
        return Err(IpcError::from(TalosError::Storage(
            "parent_task_id must be 1..=128 chars".to_owned(),
        )));
    }
    let parent = q::get_task_state(&state.db, &parent_task_id).await?;
    let supervisor_config = state.supervisor_config.lock().await.clone();
    let planning_config = state.planning_config.lock().await.clone();
    let supervisor =
        SupervisorAgent::with_config(&format!("supervisor-{parent_task_id}"), supervisor_config)
            .with_planning(planning_config);
    let goal_text = goal.unwrap_or_else(|| parent.title.clone());
    if goal_text.trim().is_empty() {
        return Err(IpcError::from(TalosError::Storage(
            "goal must not be empty".to_owned(),
        )));
    }
    let dag = plan_for_parent(
        state,
        app,
        &supervisor,
        &parent_task_id,
        &goal_text,
        parent.description_for_delegation(),
    )
    .await?;
    Ok(PlanPreviewView::from_dag(&parent_task_id, &dag))
}

/// Enforce one child's budget before execution (TAL-004).
///
/// Returns `false` (after recording + emitting `ChildBudgetExceeded`) when
/// the child's actual spend already exceeds its reservation.
async fn enforce_child_budget(
    state: &AppState,
    app: Option<&tauri::AppHandle>,
    child_task_id: &str,
) -> Result<bool, IpcError> {
    if cost_tracker::check_child_budget(&state.db, child_task_id, 0.0).await?
        == cost_tracker::ChildBudgetVerdict::Allow
    {
        return Ok(true);
    }
    let Some((reserved_usd, spent_usd)) =
        cost_tracker::get_child_reservation_spend(&state.db, child_task_id).await?
    else {
        return Ok(true);
    };
    if cost_tracker::flag_child_overspend(&state.db, child_task_id).await? {
        if let Some(app) = app {
            let _ = emit_talos_event(
                app,
                CHILD_BUDGET_EXCEEDED,
                serde_json::json!({
                    "childTaskId": child_task_id,
                    "reservedUsd": reserved_usd,
                    "spentUsd": spent_usd,
                    "checkedAt": talos_common::now_iso(),
                }),
            );
        }
    }
    Ok(false)
}

/// Outcome of `delegate_task`: children spawned plus the merged parent status.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DelegateTaskOutput {
    /// Parent task after merge.
    pub parent: q::TaskView,
    /// Child tasks spawned (in spawn order).
    pub children: Vec<q::TaskView>,
    /// Structured results per child.
    pub results: Vec<DelegationResult>,
    /// Parent status after merge (`completed` | `failed`).
    pub merged_status: String,
}

/// Ensure the parent has backing evidence so the spawn gate can allow.
///
/// Spawning is `SystemControl`/medium (write-like): the broker returns
/// `RequireEvidence` until the parent has recorded evidence. Recording the
/// delegation scope then re-evaluating is the legitimate flow (never a
/// bypass); without it every fresh parent would be undelegatable.
async fn ensure_parent_evidence(state: &AppState, parent_id: &str) -> Result<(), IpcError> {
    if evidence_store::has_evidence_for_task(&state.db, parent_id).await? {
        return Ok(());
    }
    let evidence_id = format!("delegation-scope-{parent_id}");
    // Idempotent: a retry of the same parent reuses the scope row.
    if evidence_store::insert_evidence(
        &state.db,
        evidence_store::NewEvidence {
            id: evidence_id.clone(),
            task_id: Some(parent_id.to_owned()),
            agent_id: None,
            session_id: Some("session-local".to_owned()),
            kind: "delegation_scope".to_owned(),
            label: Some("delegation scope".to_owned()),
            summary: Some(format!("delegation scope for {parent_id}")),
            uri: None,
            mime_type: None,
            payload: serde_json::json!({ "parent_task_id": parent_id }),
        },
    )
    .await
    .is_err()
    {
        return Ok(());
    }
    let _ = evidence_store::transition_evidence_status(&state.db, &evidence_id, "verified").await;
    Ok(())
}

/// Evaluate one spawn request with live ledger context.
async fn evaluate_spawn(
    state: &AppState,
    request: &policy_broker::ActionRequest,
) -> policy_broker::PolicyEvaluation {
    let has_evidence = evidence_store::has_evidence_for_task(&state.db, &request.task_id)
        .await
        .unwrap_or(false);
    state.engine.evaluate_with_context(
        request,
        &PolicyContext {
            has_evidence,
            has_compensation_plan: false,
        },
    )
}

/// Record one delegation ledger event (best effort; ledger failures surface).
async fn record_delegation_event(
    state: &AppState,
    kind: &str,
    payload: serde_json::Value,
) -> Result<(), IpcError> {
    evidence_store::append_event(
        &state.db,
        evidence_store::NewEvent {
            id: format!("delegation-{}-{}", kind, uuid::Uuid::new_v4()),
            kind: kind.to_owned(),
            payload_json: payload,
        },
    )
    .await
    .map(|_| ())
    .map_err(IpcError::from)
}

/// Execute the specialist mock for one child and persist its artifact.
async fn run_specialist(
    state: &AppState,
    role: SpecialistRole,
    child_id: &str,
    scope: &str,
    context_budget: u32,
) -> DelegationResult {
    let agent_id = format!("{child_id}-agent");
    // Workspace root for real execution (Coder children get isolated
    // worktrees/temp dirs at spawn; others run without one → mock).
    let workspace_root = {
        let workspaces = state.workspaces.lock().await;
        workspaces.workspace_root(child_id).ok()
    };
    let tools = state.tools.lock().await;
    let sandbox = tool_runtime::SandboxConfig {
        root: workspace_root
            .clone()
            .unwrap_or_else(|| state.tool_root.clone()),
        exec_enabled: state.exec_enabled,
        exec_allowlist: state.exec_allowlist.clone(),
        exec_timeout_ms: 60_000,
        max_bytes: 262_144,
        max_output_bytes: 102_400,
        allow_network: false,
        profile: sandbox::SandboxProfile::WorkspaceWrite,
        danger_approval_id: None,
        computer: state.computer.clone(),
        execution_target: tool_runtime::ExecutionTarget::Local,
        remote_runner: None,
        browser_profile_root: None,
    };
    let ctx = agent_runtime::specialists::SpecialistCtx {
        db: &state.db,
        engine: &state.engine,
        registry: &tools,
        sandbox: &sandbox,
        workspace_root,
        agent_id: agent_id.clone(),
        task_id: child_id.to_owned(),
    };
    let mut result = match role {
        SpecialistRole::Coder => {
            CoderAgent::new(&agent_id, child_id, scope, context_budget)
                .execute(&ctx)
                .await
        }
        SpecialistRole::Browser => {
            BrowserAgent::new(&agent_id, child_id, scope, context_budget)
                .execute(&ctx)
                .await
        }
        SpecialistRole::Researcher => {
            ResearchAgent::new(&agent_id, child_id, scope, context_budget)
                .execute(&ctx)
                .await
        }
    };
    // Persist the artifact body as verified evidence (hash-chained, auditable).
    if let Some(artifact) = result.artifacts.first().cloned() {
        let evidence_id = format!("{}-evidence", artifact.id);
        let payload = serde_json::json!({
            "artifactId": artifact.id,
            "kind": artifact.kind,
            "summary": artifact.summary,
            "childTaskId": child_id,
            "role": role.as_str(),
        });
        if evidence_store::insert_evidence(
            &state.db,
            evidence_store::NewEvidence {
                id: evidence_id.clone(),
                task_id: Some(child_id.to_owned()),
                agent_id: Some(agent_id.clone()),
                session_id: Some("session-local".to_owned()),
                kind: "delegation_artifact".to_owned(),
                label: Some(format!("{} artifact", role.as_str())),
                summary: Some(artifact.summary.clone()),
                uri: None,
                mime_type: None,
                payload,
            },
        )
        .await
        .is_ok()
        {
            let _ = evidence_store::transition_evidence_status(&state.db, &evidence_id, "verified")
                .await;
            result.evidence_ids.push(evidence_id.clone());
            if let Some(first) = result.artifacts.first_mut() {
                first.evidence_id = Some(evidence_id);
            }
        }
    }
    result.child_task_id = child_id.to_owned();
    result
}

/// Move a parent toward `executing` so the merge transition is valid.
async fn ensure_parent_executing(state: &AppState, parent_id: &str) -> Result<(), IpcError> {
    let view = q::get_task_state(&state.db, parent_id).await?;
    if view.status == "queued" {
        let _ = q::transition_task(&state.db, parent_id, "planning", "delegation: dispatch").await;
    }
    let view = q::get_task_state(&state.db, parent_id).await?;
    if view.status == "planning" {
        let _ =
            q::transition_task(&state.db, parent_id, "executing", "delegation: executing").await;
    }
    Ok(())
}

/// Hold a high-risk DAG plan for human approval before any child spawns.
///
/// Records a pending approval, emits `approvalRequested`, and returns the
/// parent untouched with `merged_status = "awaiting_approval"`. The UI
/// "Approve Plan" button grants the approval; the user then re-invokes
/// `delegate_task` to execute the approved plan.
async fn hold_for_plan_approval(
    state: &AppState,
    app: Option<&tauri::AppHandle>,
    supervisor: &SupervisorAgent,
    parent_task_id: &str,
    dag: &PlannedDag,
) -> Result<DelegateTaskOutput, IpcError> {
    use policy_broker::{ActionRequest, Capability, PolicyDecision, RiskLevel};
    let request = ActionRequest {
        id: format!("plan-approval-{parent_task_id}"),
        agent_id: supervisor.id.clone(),
        task_id: parent_task_id.to_owned(),
        capability: Capability::SystemControl,
        intent: format!(
            "execute high-risk DAG plan ({} steps, {} need approval)",
            dag.steps.len(),
            dag.steps.iter().filter(|s| s.requires_approval).count()
        ),
        target: parent_task_id.to_owned(),
        payload_json: serde_json::json!({
            "parent_task_id": parent_task_id,
            "planning_method": dag.method.as_str(),
            "steps": dag.steps.iter().map(|s| serde_json::json!({
                "step_id": s.step_id,
                "role": s.role.as_str(),
                "goal": s.scope,
                "requires_approval": s.requires_approval,
            })).collect::<Vec<_>>(),
        }),
        risk_level: RiskLevel::High,
        reversible: false,
        requested_at: chrono::Utc::now(),
    };
    let approval = {
        let mut approvals = state.approvals.lock().await;
        approvals.request_approval(
            &request,
            PolicyDecision::RequireHumanReview,
            "high-risk plan step requires human approval before execution",
            &[],
        )
    };
    record_delegation_event(
        state,
        "planning.approval_requested",
        serde_json::json!({
            "parent_task_id": parent_task_id,
            "approval_id": approval.approval_id,
        }),
    )
    .await?;
    if let Some(app) = app {
        let _ = emit_talos_event(
            app,
            crate::events::APPROVAL_REQUESTED,
            serde_json::json!({
                "approvalId": approval.approval_id,
                "taskId": parent_task_id,
                "approval": approval,
            }),
        );
    }
    let parent = q::get_task_state(&state.db, parent_task_id).await?;
    Ok(DelegateTaskOutput {
        parent,
        children: Vec::new(),
        results: Vec::new(),
        merged_status: "awaiting_approval".to_owned(),
    })
}

/// One executed (or canceled) child: queue view, structured result, and its
/// budget reservation for settlement.
struct ChildExecution {
    child: q::TaskView,
    result: DelegationResult,
    reservation: (String, Option<String>),
}

/// Execute one delegation request end to end: policy gate → spawn → workspace
/// → budget reservation → queue execution → specialist result (every step
/// policy-gated; the Supervisor can never override a child decision).
#[allow(clippy::too_many_arguments)]
async fn execute_delegation_request(
    state: &AppState,
    app: Option<&tauri::AppHandle>,
    supervisor: &SupervisorAgent,
    parent_task_id: &str,
    parent_priority: &str,
    parent_allowlist: &[String],
    parent_depth: usize,
    existing_before: usize,
    request: &agent_runtime::delegation::DelegationRequest,
) -> Result<ChildExecution, IpcError> {
    supervisor.validate_spawn(existing_before, parent_depth, parent_allowlist, request)?;
    // Policy gate: the Supervisor can never override a child decision.
    let spawn_request = supervisor.spawn_policy_request(parent_task_id, &request.child_task_id);
    let evaluation = evaluate_spawn(state, &spawn_request).await;
    // Audit the spawn request itself.
    let record = evidence_store::NewActionRecord {
        id: spawn_request.id.clone(),
        agent_id: spawn_request.agent_id.clone(),
        task_id: spawn_request.task_id.clone(),
        capability: format!("{:?}", spawn_request.capability),
        intent: spawn_request.intent.clone(),
        target: spawn_request.target.clone(),
        payload: spawn_request.payload_json.clone(),
        risk_level: format!("{:?}", spawn_request.risk_level),
        reversible: spawn_request.reversible,
        requested_at: spawn_request.requested_at.to_rfc3339(),
    };
    let _ = evidence_store::record_action_request(
        &state.db,
        &record,
        crate::commands::POLICY_VERSION,
        &format!("{:?}", evaluation.decision),
        &evaluation.reason,
        &evaluation.required_checks,
    )
    .await;
    if evaluation.decision != policy_broker::PolicyDecision::Allow {
        return Err(IpcError::from(TalosError::Storage(format!(
            "delegation denied by policy: {:?} ({}); record evidence for the parent first",
            evaluation.decision, evaluation.reason
        ))));
    }
    // Spawn the child row (deterministic id, parent-linked).
    let seq = {
        let mut runtime = state.runtime.lock().await;
        runtime.next_queue_seq()
    };
    let mut seq_holder = seq;
    let child = q::submit_task_with_id(
        &state.db,
        &mut seq_holder,
        q::SubmitParams {
            title: format!("{} [{}]", request.scope, request.agent_role.as_str()),
            description: Some(request.scope.clone()),
            priority: parent_priority.to_owned(),
            timeout_ms: request.deadline_ms,
            max_attempts: 2,
            scenario: "quick_success".to_owned(),
            parent_task_id: Some(parent_task_id.to_owned()),
        },
        Some(&request.child_task_id),
    )
    .await?;
    // Coder children get an isolated workspace (git worktree or temp dir):
    // the workspace root becomes their only filesystem root for all
    // subsequent file/shell/git/code tool calls.
    if request.agent_role == SpecialistRole::Coder {
        let workspace = {
            let workspaces = state.workspaces.lock().await;
            workspaces.create_workspace(&child.id, None).await
        };
        match workspace {
            Ok(ws) => {
                record_delegation_event(
                    state,
                    "delegation.workspace",
                    serde_json::json!({
                        "parent_task_id": parent_task_id,
                        "child_task_id": child.id,
                        "workspace": ws.root.to_string_lossy(),
                    }),
                )
                .await?;
                if let Some(app) = app {
                    let _ = emit_talos_event(
                        app,
                        crate::events::WORKSPACE_UPDATED,
                        serde_json::json!({
                            "taskId": child.id,
                            "path": ws.root.to_string_lossy(),
                            "status": "created",
                            "checkedAt": talos_common::now_iso(),
                        }),
                    );
                }
            }
            Err(e) => {
                eprintln!("[talos] workspace create failed for {}: {e}", child.id);
            }
        }
    }
    record_delegation_event(
        state,
        "delegation.spawned",
        serde_json::json!({
            "parent_task_id": parent_task_id,
            "child_task_id": child.id,
            "role": request.agent_role.as_str(),
        }),
    )
    .await?;
    if let Some(app) = app {
        let _ = emit_talos_event(
            app,
            AGENT_DELEGATED,
            serde_json::json!({
                "parentTaskId": parent_task_id,
                "childTaskId": child.id,
                "role": request.agent_role.as_str(),
            }),
        );
        let _ = emit_talos_event(
            app,
            CHILD_TASK_SPAWNED,
            serde_json::json!({
                "parentTaskId": parent_task_id,
                "childTaskId": child.id,
                "role": request.agent_role.as_str(),
            }),
        );
    }
    // Reserve the child's budget share and set its task cap (TAL-004):
    // the LLM gateway and tool executor deny over-cap calls, while the
    // reservation ledger drives settlement and explicit checks.
    let reservation_id = match request.budget_usd {
        Some(share) => {
            let id = cost_tracker::reserve_budget(
                &state.db,
                parent_task_id,
                &request.child_task_id,
                share,
            )
            .await?;
            evidence_store::set_task_budget(&state.db, &request.child_task_id, Some(share)).await?;
            Some(id)
        }
        None => None,
    };
    // Execute the child through the durable queue (policy-gated worker).
    let done = if !enforce_child_budget(state, app, &request.child_task_id).await? {
        // No direct queued → failed edge: route through planning.
        let planning = q::transition_task(
            &state.db,
            &request.child_task_id,
            "planning",
            "delegation: budget gate",
        )
        .await?;
        q::transition_task(
            &state.db,
            &planning.id,
            "failed",
            "child budget exceeded before execution",
        )
        .await?
    } else {
        q::execute_task(
            &state.db,
            &state.engine,
            &request.child_task_id,
            &q::CancelToken::new(),
        )
        .await?
    };
    // Specialist mock produces the structured artifact for this slice.
    let mut result = run_specialist(
        state,
        request.agent_role,
        &child.id,
        &request.scope,
        request.context_budget,
    )
    .await;
    // Queue verdict rules: a failed child marks the delegation failed.
    if done.status != "completed" {
        result.status = done.status.clone();
    }
    // Persist the child's execution fidelity (never upgraded without proof).
    let _ = evidence_store::set_task_fidelity(&state.db, &child.id, result.fidelity.as_str()).await;
    if let Some(app) = app {
        let payload = serde_json::json!({
            "parentTaskId": parent_task_id,
            "childTaskId": child.id,
            "status": result.status,
            "fidelity": result.fidelity.as_str(),
            "limitations": result.limitations,
            "error": done.last_error.clone().unwrap_or_else(|| result.status.clone()),
        });
        if result.status == "completed" {
            let _ = emit_talos_event(app, CHILD_TASK_COMPLETED, payload);
        } else {
            let _ = emit_talos_event(app, CHILD_TASK_FAILED, payload);
        }
    }
    record_delegation_event(
        state,
        if result.status == "completed" {
            "delegation.child_completed"
        } else {
            "delegation.child_failed"
        },
        serde_json::json!({
            "parent_task_id": parent_task_id,
            "child_task_id": child.id,
            "status": result.status,
            "fidelity": result.fidelity.as_str(),
            "limitations": result.limitations,
        }),
    )
    .await?;
    Ok(ChildExecution {
        child: done,
        result,
        reservation: (request.child_task_id.clone(), reservation_id),
    })
}

/// Cancel one DAG step whose dependencies did not complete: the child row is
/// still spawned (deterministic id, auditable) but moves straight to
/// `canceled` without executing any tools.
async fn cancel_blocked_child(
    state: &AppState,
    app: Option<&tauri::AppHandle>,
    parent_task_id: &str,
    parent_priority: &str,
    request: &agent_runtime::delegation::DelegationRequest,
    blocked_on: &[String],
) -> Result<ChildExecution, IpcError> {
    let seq = {
        let mut runtime = state.runtime.lock().await;
        runtime.next_queue_seq()
    };
    let mut seq_holder = seq;
    let child = q::submit_task_with_id(
        &state.db,
        &mut seq_holder,
        q::SubmitParams {
            title: format!("{} [{}]", request.scope, request.agent_role.as_str()),
            description: Some(request.scope.clone()),
            priority: parent_priority.to_owned(),
            timeout_ms: request.deadline_ms,
            max_attempts: 2,
            scenario: "quick_success".to_owned(),
            parent_task_id: Some(parent_task_id.to_owned()),
        },
        Some(&request.child_task_id),
    )
    .await?;
    q::cancel_task(&state.db, &request.child_task_id).await?;
    let done = q::execute_task(
        &state.db,
        &state.engine,
        &request.child_task_id,
        &q::CancelToken::new(),
    )
    .await?;
    let detail = format!(
        "canceled: dependencies did not complete [{}]",
        blocked_on.join(", ")
    );
    let result = DelegationResult {
        child_task_id: child.id.clone(),
        status: done.status.clone(),
        artifacts: Vec::new(),
        evidence_ids: Vec::new(),
        limitations: vec![detail.clone()],
        remaining_risks: vec!["dependent work incomplete".to_owned()],
        fidelity: talos_common::ExecutionFidelity::Mock,
    };
    let _ = evidence_store::set_task_fidelity(&state.db, &child.id, result.fidelity.as_str()).await;
    if let Some(app) = app {
        let _ = emit_talos_event(
            app,
            CHILD_TASK_FAILED,
            serde_json::json!({
                "parentTaskId": parent_task_id,
                "childTaskId": child.id,
                "status": result.status,
                "fidelity": result.fidelity.as_str(),
                "limitations": result.limitations,
                "error": detail,
            }),
        );
    }
    record_delegation_event(
        state,
        "delegation.child_canceled",
        serde_json::json!({
            "parent_task_id": parent_task_id,
            "child_task_id": child.id,
            "status": result.status,
            "blocked_on": blocked_on,
        }),
    )
    .await?;
    Ok(ChildExecution {
        child: done,
        result,
        reservation: (request.child_task_id.clone(), None),
    })
}

/// Inner delegation (shared by the command and tests; emits when `app` given).
#[allow(clippy::too_many_arguments)]
pub async fn delegate_task_inner(
    state: &AppState,
    app: Option<&tauri::AppHandle>,
    parent_task_id: String,
    goal: Option<String>,
    role: Option<String>,
    scope: Option<String>,
    deadline_ms: Option<i64>,
    context_budget: Option<u32>,
) -> Result<DelegateTaskOutput, IpcError> {
    if parent_task_id.trim().is_empty() || parent_task_id.len() > 128 {
        return Err(IpcError::from(TalosError::Storage(
            "parent_task_id must be 1..=128 chars".to_owned(),
        )));
    }
    let parent = q::get_task_state(&state.db, &parent_task_id).await?;
    if agent_runtime::lifecycle::is_terminal(&parent.status) {
        return Err(IpcError::from(TalosError::Storage(
            "cannot delegate a terminal parent task".to_owned(),
        )));
    }
    let parent_depth = evidence_store::task_depth(&state.db, &parent_task_id).await?;
    let existing_children = evidence_store::list_children(&state.db, &parent_task_id)
        .await?
        .len();
    // Supervisor tuning is live state (TAL-005): update_supervisor_config
    // takes effect on the next delegation without a restart.
    let supervisor_config = state.supervisor_config.lock().await.clone();
    let planning_config = state.planning_config.lock().await.clone();
    let supervisor =
        SupervisorAgent::with_config(&format!("supervisor-{parent_task_id}"), supervisor_config)
            .with_planning(planning_config);

    // Plan: an explicit role wins (single scoped child); otherwise the
    // Supervisor plans an LLM-driven DAG validated by deterministic gates,
    // falling back to keyword decomposition (always labeled).
    let mut planned_dag: Option<PlannedDag> = None;
    let mut requests = if let Some(role_name) = role.as_deref() {
        let parsed = SpecialistRole::parse(role_name).map_err(IpcError::from)?;
        let planned = vec![agent_runtime::delegation::PlannedSubtask {
            role: parsed,
            scope: scope.clone().unwrap_or_else(|| parent.title.clone()),
        }];
        supervisor.plan_delegations(
            &parent_task_id,
            parent_depth,
            &planned,
            evidence_store::get_task_budget(&state.db, &parent_task_id).await?,
        )
    } else {
        let goal_text = goal.clone().unwrap_or_else(|| parent.title.clone());
        if goal_text.trim().is_empty() {
            return Err(IpcError::from(TalosError::Storage(
                "goal must not be empty".to_owned(),
            )));
        }
        let dag = plan_for_parent(
            state,
            app,
            &supervisor,
            &parent_task_id,
            &goal_text,
            parent.description_for_delegation(),
        )
        .await?;
        if dag.steps.is_empty() {
            return Err(IpcError::from(TalosError::Storage(
                "nothing to delegate".to_owned(),
            )));
        }
        // High-risk plans hold for human approval before any child spawns.
        if dag.requires_approval {
            return hold_for_plan_approval(state, app, &supervisor, &parent_task_id, &dag).await;
        }
        let planned = supervisor.plan_dag_delegations(
            &parent_task_id,
            parent_depth,
            &dag,
            evidence_store::get_task_budget(&state.db, &parent_task_id).await?,
        );
        planned_dag = Some(dag);
        planned
    };
    if requests.is_empty() {
        return Err(IpcError::from(TalosError::Storage(
            "nothing to delegate".to_owned(),
        )));
    }
    // Caller overrides apply to single-child delegations.
    if requests.len() == 1 {
        if let Some(deadline) = deadline_ms {
            if !(100..=3_600_000).contains(&deadline) {
                return Err(IpcError::from(TalosError::Storage(
                    "deadlineMs out of range".to_owned(),
                )));
            }
            requests[0].deadline_ms = deadline;
        }
        if let Some(budget) = context_budget {
            if budget == 0 || budget > 128_000 {
                return Err(IpcError::from(TalosError::Storage(
                    "contextBudget out of range".to_owned(),
                )));
            }
            requests[0].context_budget = budget;
        }
        if let Some(custom) = scope.clone() {
            if custom.trim().is_empty() || custom.len() > 2048 {
                return Err(IpcError::from(TalosError::Storage(
                    "scope must be 1..=2048 chars".to_owned(),
                )));
            }
            requests[0].scope = custom;
            requests[0].tool_allowlist = requests[0].agent_role.tool_allowlist();
        }
    }

    ensure_parent_evidence(state, &parent_task_id).await?;
    let parent_allowlist = SupervisorAgent::parent_authority();

    let mut children = Vec::new();
    let mut results = Vec::new();
    // Child task id → budget reservation (settled after execution).
    let mut reservations: Vec<(String, Option<String>)> = Vec::new();
    if let Some(dag) = planned_dag {
        // DAG execution: levels run in topological order (sequentially
        // within a level, bounded by the validated fan-out); a step whose
        // dependencies did not complete is canceled with its dependents.
        let mut status_by_step: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for level in &dag.levels {
            for step_id in level {
                let Some(index) = dag.steps.iter().position(|s| &s.step_id == step_id) else {
                    continue;
                };
                let request = &requests[index];
                let blocked: Vec<String> = dag.steps[index]
                    .depends_on
                    .iter()
                    .filter(|dep| status_by_step.get(*dep).map(String::as_str) != Some("completed"))
                    .cloned()
                    .collect();
                if !blocked.is_empty() {
                    let canceled = cancel_blocked_child(
                        state,
                        app,
                        &parent_task_id,
                        &parent.priority,
                        request,
                        &blocked,
                    )
                    .await?;
                    status_by_step.insert(step_id.clone(), canceled.result.status.clone());
                    reservations.push(canceled.reservation);
                    children.push(canceled.child);
                    results.push(canceled.result);
                    continue;
                }
                let done = execute_delegation_request(
                    state,
                    app,
                    &supervisor,
                    &parent_task_id,
                    &parent.priority,
                    &parent_allowlist,
                    parent_depth,
                    existing_children + children.len(),
                    request,
                )
                .await?;
                status_by_step.insert(step_id.clone(), done.result.status.clone());
                reservations.push(done.reservation);
                children.push(done.child);
                results.push(done.result);
            }
        }
    } else {
        for request in &requests {
            let done = execute_delegation_request(
                state,
                app,
                &supervisor,
                &parent_task_id,
                &parent.priority,
                &parent_allowlist,
                parent_depth,
                existing_children + children.len(),
                request,
            )
            .await?;
            reservations.push(done.reservation);
            children.push(done.child);
            results.push(done.result);
        }
    }

    // Settle budget reservations: actual spend is journaled, the unused share
    // returns to the parent implicitly (parent caps never included it), and
    // overspend is recorded. Settlement never fails the delegation: execution
    // already happened, so failures are logged with a ledger event.
    for (child_task_id, reservation_id) in &reservations {
        if let Some(reservation_id) = reservation_id {
            let actual = cost_tracker::total_task_spend(&state.db, child_task_id)
                .await
                .unwrap_or(0.0);
            if let Err(e) = cost_tracker::settle_budget(&state.db, reservation_id, actual).await {
                eprintln!("[talos] budget settle failed for {child_task_id}: {e}");
                let _ = record_delegation_event(
                    state,
                    "delegation.settle_failed",
                    serde_json::json!({
                        "parent_task_id": parent_task_id,
                        "child_task_id": child_task_id,
                        "error": e.to_string(),
                    }),
                )
                .await;
            }
        }
    }

    // Merge: all children completed → parent completed, else failed.
    // Parent fidelity is the minimum of the children (never upgraded).
    let outcome = supervisor.merge(&parent_task_id, &results);
    let _ =
        evidence_store::set_task_fidelity(&state.db, &parent_task_id, outcome.fidelity.as_str())
            .await;
    ensure_parent_executing(state, &parent_task_id).await?;
    let target = if outcome.status == "completed" {
        "completed"
    } else {
        "failed"
    };
    let merged =
        match q::transition_task(&state.db, &parent_task_id, target, &outcome.summary).await {
            Ok(view) => view,
            Err(_) => q::get_task_state(&state.db, &parent_task_id).await?,
        };
    record_delegation_event(
        state,
        "delegation.merged",
        serde_json::json!({
            "parent_task_id": parent_task_id,
            "status": outcome.status,
            "artifact_count": outcome.artifacts.len(),
            "fidelity": outcome.fidelity.as_str(),
        }),
    )
    .await?;
    if let Some(app) = app {
        let _ = emit_talos_event(
            app,
            RESULTS_MERGED,
            serde_json::json!({
                "parentTaskId": parent_task_id,
                "status": outcome.status,
                "artifactCount": outcome.artifacts.len(),
                "fidelity": outcome.fidelity.as_str(),
            }),
        );
        let _ = emit_talos_event(
            app,
            TASK_UPDATED,
            serde_json::json!({ "task": {
                "id": merged.id,
                "title": merged.title,
                "status": merged.status,
            } }),
        );
    }
    Ok(DelegateTaskOutput {
        parent: merged.clone(),
        children,
        results,
        merged_status: outcome.status,
    })
}

/// Helper: parent description for decomposition (avoids cloning the view).
trait ParentDescription {
    /// Optional detail for deterministic decomposition.
    fn description_for_delegation(&self) -> Option<&str>;
}

impl ParentDescription for q::TaskView {
    fn description_for_delegation(&self) -> Option<&str> {
        None
    }
}

/// `delegate_task` — decompose a parent and spawn scoped sub-agents.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn delegate_task(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    parent_task_id: String,
    goal: Option<String>,
    role: Option<String>,
    scope: Option<String>,
    deadline_ms: Option<i64>,
    context_budget: Option<u32>,
) -> Result<DelegateTaskOutput, IpcError> {
    delegate_task_inner(
        &state,
        Some(&app),
        parent_task_id,
        goal,
        role,
        scope,
        deadline_ms,
        context_budget,
    )
    .await
}

/// `get_task_tree` — parent-child hierarchy rooted at `task_id`.
#[tauri::command]
pub async fn get_task_tree(
    state: tauri::State<'_, AppState>,
    task_id: String,
) -> Result<q::TaskTreeView, IpcError> {
    Ok(q::get_task_tree(&state.db, &task_id).await?)
}

/// `cancel_subtree` — cancel a parent and all descendants.
#[tauri::command]
pub async fn cancel_subtree(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    task_id: String,
) -> Result<q::TaskView, IpcError> {
    let canceled = q::cancel_subtree(&state.db, &task_id).await?;
    let view = q::get_task_state(&state.db, &task_id).await?;
    let _ = canceled;
    // Best-effort workspace cleanup for every task in the subtree.
    if let Ok(tree) = q::get_task_tree(&state.db, &task_id).await {
        let mut stack = vec![tree];
        while let Some(node) = stack.pop() {
            for child in node.children {
                stack.push(child);
            }
            let workspaces = state.workspaces.lock().await;
            let _ = workspaces.cleanup(&node.task.id).await;
        }
    }
    let _ = emit_talos_event(
        &app,
        TASK_UPDATED,
        serde_json::json!({ "task": {
            "id": view.id,
            "title": view.title,
            "status": view.status,
        } }),
    );
    Ok(view)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_state() -> AppState {
        AppState::new_in_memory()
            .await
            .expect("in-memory state builds")
    }

    async fn submit_parent(state: &AppState, title: &str) -> q::TaskView {
        let seq = {
            let mut runtime = state.runtime.lock().await;
            runtime.next_queue_seq()
        };
        let mut holder = seq;
        q::submit_task(
            &state.db,
            &mut holder,
            q::SubmitParams {
                title: title.to_owned(),
                description: None,
                priority: "normal".to_owned(),
                timeout_ms: 5_000,
                max_attempts: 2,
                scenario: "quick_success".to_owned(),
                parent_task_id: None,
            },
        )
        .await
        .expect("submit")
    }

    #[tokio::test]
    async fn delegates_code_goal_to_coder_child() {
        let state = test_state().await;
        let parent = submit_parent(&state, "fix the code path").await;
        let out = delegate_task_inner(
            &state,
            None,
            parent.id.clone(),
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .expect("delegate");
        assert_eq!(out.children.len(), 1);
        assert!(out.children[0].id.contains("coder"));
        assert_eq!(
            out.children[0].parent_task_id.as_deref(),
            Some(parent.id.as_str())
        );
        assert_eq!(out.merged_status, "completed");
        assert_eq!(out.parent.status, "completed");
    }

    #[tokio::test]
    async fn preview_plan_does_not_execute() {
        let state = test_state().await;
        let parent = submit_parent(&state, "fix the code path").await;
        let preview = preview_plan_inner(&state, None, parent.id.clone(), None)
            .await
            .expect("preview");
        assert_eq!(preview.parent_task_id, parent.id);
        // No provider key in tests: labeled keyword fallback.
        assert_eq!(preview.method, "keyword");
        assert!(!preview.steps.is_empty());
        assert!(preview.valid);
        assert!(preview.fallback_reason.is_some());
        // Preview spawns nothing: the parent stays queued.
        let after = q::get_task_state(&state.db, &parent.id)
            .await
            .expect("state");
        assert_eq!(after.status, "queued");
        // ...but the plan artifact is evidenced.
        let log = evidence_store::list_recent_log(&state.db, 50)
            .await
            .expect("ledger lists");
        assert!(log.iter().any(|e| e.kind == "planning.completed"));
    }

    #[tokio::test]
    async fn update_planning_config_validates_and_applies() {
        let state = test_state().await;
        assert!(
            update_planning_config_inner(&state, "".to_owned(), 30_000, true)
                .await
                .is_err()
        );
        assert!(
            update_planning_config_inner(&state, "gpt-4o-mini".to_owned(), 500, true)
                .await
                .is_err()
        );
        let view = update_planning_config_inner(&state, "ollama/llama3".to_owned(), 10_000, false)
            .await
            .expect("valid config applies");
        assert_eq!(view.model, "ollama/llama3");
        assert_eq!(view.timeout_ms, 10_000);
        assert!(!view.fallback_enabled);
        let current = get_planning_config_inner(&state).await;
        assert_eq!(current.model, "ollama/llama3");
        let log = evidence_store::list_recent_log(&state.db, 20)
            .await
            .expect("ledger lists");
        assert!(log.iter().any(|e| e.kind == "planning.config_updated"));
    }

    #[tokio::test]
    async fn high_risk_plan_holds_for_approval() {
        use agent_runtime::planner::PlanningMethod;
        let state = test_state().await;
        let parent = submit_parent(&state, "migrate the production database").await;
        let supervisor = SupervisorAgent::with_config(
            &format!("supervisor-{}", parent.id),
            SupervisorAgent::new("x").config.clone(),
        );
        let mut dag = agent_runtime::planner::keyword_fallback_dag(
            "migrate the production database",
            None,
            None,
        );
        dag.method = PlanningMethod::Llm;
        dag.requires_approval = true;
        for step in &mut dag.steps {
            step.requires_approval = true;
        }
        let out = hold_for_plan_approval(&state, None, &supervisor, &parent.id, &dag)
            .await
            .expect("hold");
        assert_eq!(out.merged_status, "awaiting_approval");
        assert!(out.children.is_empty());
        let pending = state.approvals.lock().await.pending();
        assert!(pending.iter().any(|a| a.task_id == parent.id));
        // Nothing spawned: no children exist.
        let children = evidence_store::list_children(&state.db, &parent.id)
            .await
            .expect("children list");
        assert!(children.is_empty());
    }

    #[tokio::test]
    async fn blocked_dependent_is_canceled_without_tools() {
        let state = test_state().await;
        let parent = submit_parent(&state, "research then code").await;
        let request = agent_runtime::delegation::DelegationRequest {
            parent_task_id: parent.id.clone(),
            child_task_id: format!("{}-step-b-coder", parent.id),
            agent_role: SpecialistRole::Coder,
            scope: "apply the fix".to_owned(),
            tool_allowlist: vec!["fs.read".to_owned()],
            context_budget: 4000,
            deadline_ms: 30_000,
            depth: 2,
            budget_usd: None,
        };
        let blocked = vec!["a".to_owned()];
        let done = cancel_blocked_child(&state, None, &parent.id, "normal", &request, &blocked)
            .await
            .expect("cancel");
        assert_eq!(done.result.status, "canceled");
        assert!(done
            .result
            .limitations
            .iter()
            .any(|l| l.contains("dependencies did not complete")));
        assert!(done.child.id.contains("-step-b-coder"));
    }

    #[tokio::test]
    async fn delegates_multi_domain_to_multiple_children() {
        let state = test_state().await;
        let parent = submit_parent(&state, "code the page and browser test the web flow").await;
        let out = delegate_task_inner(
            &state,
            None,
            parent.id.clone(),
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .expect("delegate");
        assert!(out.children.len() >= 2);
        assert_eq!(out.merged_status, "completed");
    }

    #[tokio::test]
    async fn rejects_grandchild_spawn() {
        let state = test_state().await;
        let parent = submit_parent(&state, "research the registry depth").await;
        // Queued child (depth 2) that has not been merged to terminal.
        let seq = {
            let mut runtime = state.runtime.lock().await;
            runtime.next_queue_seq()
        };
        let mut holder = seq;
        let child = q::submit_task_with_id(
            &state.db,
            &mut holder,
            q::SubmitParams {
                title: "child slice".to_owned(),
                description: None,
                priority: "normal".to_owned(),
                timeout_ms: 5_000,
                max_attempts: 2,
                scenario: "quick_success".to_owned(),
                parent_task_id: Some(parent.id.clone()),
            },
            Some(&format!("{}-child-1-researcher", parent.id)),
        )
        .await
        .expect("child");
        let err = delegate_task_inner(&state, None, child.id, None, None, None, None, None).await;
        assert!(err.is_err());
        let message = err.unwrap_err().message;
        assert!(
            message.contains("depth") || message.contains("grandchild"),
            "unexpected: {message}"
        );
    }

    #[tokio::test]
    async fn update_supervisor_config_validates_and_applies() {
        let state = test_state().await;
        // Invalid ranges are rejected; live config untouched.
        assert!(update_supervisor_config_inner(&state, 0, 2).await.is_err());
        assert!(update_supervisor_config_inner(&state, 2, 0).await.is_err());
        assert!(update_supervisor_config_inner(&state, 17, 2).await.is_err());
        assert!(update_supervisor_config_inner(&state, 2, 5).await.is_err());
        {
            let config = state.supervisor_config.lock().await;
            assert_eq!(config.max_fanout, 4);
            assert_eq!(config.max_depth, 2);
        }
        // Valid update applies and is evidence-logged.
        let view = update_supervisor_config_inner(&state, 2, 1)
            .await
            .expect("valid config applies");
        assert_eq!(view.max_fanout, 2);
        assert_eq!(view.max_depth, 1);
        {
            let config = state.supervisor_config.lock().await;
            assert_eq!(config.max_fanout, 2);
            assert_eq!(config.max_depth, 1);
        }
        let log = evidence_store::list_recent_log(&state.db, 20)
            .await
            .expect("ledger lists");
        assert!(log.iter().any(|e| e.kind == "supervisor.config_updated"));
        // A max_depth of 1 denies every spawn through the live config.
        let parent = submit_parent(&state, "research the depth limit").await;
        let err = delegate_task_inner(&state, None, parent.id, None, None, None, None, None)
            .await
            .expect_err("depth-1 config denies delegation");
        assert!(err.message.contains("depth"), "unexpected: {}", err.message);
    }

    #[tokio::test]
    async fn delegation_propagates_and_settles_child_budgets() {
        // TAL-004 integration: $1.00 parent → 2 children share $0.50 each,
        // reservations settle on completion.
        let state = test_state().await;
        let parent = submit_parent(&state, "code the page and browser test the web flow").await;
        evidence_store::set_task_budget(&state.db, &parent.id, Some(1.0))
            .await
            .expect("parent budget sets");
        let out = delegate_task_inner(
            &state,
            None,
            parent.id.clone(),
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .expect("delegate");
        assert!(out.children.len() >= 2);
        for child in &out.children {
            let cap = evidence_store::get_task_budget(&state.db, &child.id)
                .await
                .expect("budget reads");
            assert_eq!(cap, Some(0.5));
            // Settled after completion: no active reservation remains.
            assert!(
                evidence_store::get_active_reservation_for_child(&state.db, &child.id)
                    .await
                    .expect("reservation reads")
                    .is_none()
            );
        }
        let log = evidence_store::list_recent_log(&state.db, 50)
            .await
            .expect("ledger lists");
        assert!(log.iter().any(|e| e.kind == "budget.reserved"));
        assert!(log.iter().any(|e| e.kind == "budget.settled"));
    }

    #[tokio::test]
    async fn cancel_subtree_cancels_children() {
        let state = test_state().await;
        // Parent that stays queued (no execution yet) + delegated children.
        let parent = submit_parent(&state, "research cancel tree").await;
        // Manually add a child without executing the parent merge.
        let seq = {
            let mut runtime = state.runtime.lock().await;
            runtime.next_queue_seq()
        };
        let mut holder = seq;
        let child = q::submit_task_with_id(
            &state.db,
            &mut holder,
            q::SubmitParams {
                title: "child".to_owned(),
                description: None,
                priority: "normal".to_owned(),
                timeout_ms: 5_000,
                max_attempts: 2,
                scenario: "quick_success".to_owned(),
                parent_task_id: Some(parent.id.clone()),
            },
            Some(&format!("{}-child-manual", parent.id)),
        )
        .await
        .expect("child");
        let count = q::cancel_subtree(&state.db, &parent.id)
            .await
            .expect("cancel");
        assert!(count >= 2);
        let after = q::get_task_state(&state.db, &child.id)
            .await
            .expect("child state");
        assert_eq!(after.status, "canceling");
    }
}
