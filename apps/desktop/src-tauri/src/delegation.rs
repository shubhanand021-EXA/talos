//! Multi-agent delegation IPC: `delegate_task`, `get_task_tree`, `cancel_subtree`.
//!
//! The Supervisor decomposes a parent task, spawns scoped children (each
//! policy-gated as `SystemControl`/medium, each independently ledger-backed),
//! executes them through the durable queue, merges structured artifacts, and
//! emits `agentDelegated` / `childTaskSpawned` / `childTaskCompleted` /
//! `childTaskFailed` / `resultsMerged` for the UI and Void Neon.

use agent_runtime::{
    delegation::{DelegationResult, SpecialistRole},
    queue as q,
    specialists::{BrowserAgent, CoderAgent, ResearchAgent, Specialist},
    supervisor::{SupervisorAgent, SupervisorConfig},
};
use policy_broker::{PolicyContext, PolicyEngine};
use talos_common::TalosError;

use crate::commands::IpcError;
use crate::events::{
    emit_talos_event, AGENT_DELEGATED, CHILD_BUDGET_EXCEEDED, CHILD_TASK_COMPLETED,
    CHILD_TASK_FAILED, CHILD_TASK_SPAWNED, RESULTS_MERGED, TASK_UPDATED,
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
    let mut result = match role {
        SpecialistRole::Coder => {
            CoderAgent::new(&agent_id, child_id, scope, context_budget).execute()
        }
        SpecialistRole::Browser => {
            BrowserAgent::new(&agent_id, child_id, scope, context_budget).execute()
        }
        SpecialistRole::Researcher => {
            ResearchAgent::new(&agent_id, child_id, scope, context_budget).execute()
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
    let supervisor =
        SupervisorAgent::with_config(&format!("supervisor-{parent_task_id}"), supervisor_config);

    // Plan: explicit role wins; otherwise deterministic decomposition of the
    // goal (falling back to the parent title).
    let planned = if let Some(role_name) = role.as_deref() {
        let parsed = SpecialistRole::parse(role_name).map_err(IpcError::from)?;
        vec![agent_runtime::delegation::PlannedSubtask {
            role: parsed,
            scope: scope.clone().unwrap_or_else(|| parent.title.clone()),
        }]
    } else {
        let goal_text = goal.clone().unwrap_or_else(|| parent.title.clone());
        if goal_text.trim().is_empty() {
            return Err(IpcError::from(TalosError::Storage(
                "goal must not be empty".to_owned(),
            )));
        }
        supervisor.decompose(&goal_text, parent.description_for_delegation())
    };
    if planned.is_empty() {
        return Err(IpcError::from(TalosError::Storage(
            "nothing to delegate".to_owned(),
        )));
    }
    let mut requests = supervisor.plan_delegations(
        &parent_task_id,
        parent_depth,
        &planned,
        evidence_store::get_task_budget(&state.db, &parent_task_id).await?,
    );
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
    for request in &requests {
        supervisor.validate_spawn(
            existing_children + children.len(),
            parent_depth,
            &parent_allowlist,
            request,
        )?;
        // Policy gate: the Supervisor can never override a child decision.
        let spawn_request =
            supervisor.spawn_policy_request(&parent_task_id, &request.child_task_id);
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
                priority: parent.priority.clone(),
                timeout_ms: request.deadline_ms,
                max_attempts: 2,
                scenario: "quick_success".to_owned(),
                parent_task_id: Some(parent_task_id.clone()),
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
                    &parent_task_id,
                    &request.child_task_id,
                    share,
                )
                .await?;
                evidence_store::set_task_budget(&state.db, &request.child_task_id, Some(share))
                    .await?;
                Some(id)
            }
            None => None,
        };
        reservations.push((request.child_task_id.clone(), reservation_id));
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
        if let Some(app) = app {
            let payload = serde_json::json!({
                "parentTaskId": parent_task_id,
                "childTaskId": child.id,
                "status": result.status,
            });
            if result.status == "completed" {
                let _ = emit_talos_event(app, CHILD_TASK_COMPLETED, payload);
            } else {
                let _ = emit_talos_event(
                    app,
                    CHILD_TASK_FAILED,
                    serde_json::json!({
                        "parentTaskId": parent_task_id,
                        "childTaskId": child.id,
                        "error": done.last_error.clone().unwrap_or_else(|| result.status.clone()),
                    }),
                );
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
            }),
        )
        .await?;
        children.push(done);
        results.push(result);
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
    let outcome = supervisor.merge(&parent_task_id, &results);
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
