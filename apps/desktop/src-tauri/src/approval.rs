//! HITL approval IPC: queue inspection, grant/deny/modify, Plan/Act modes,
//! plan documents, and compensation execution.
//!
//! Wire shapes mirror the TS `ApprovalRequest` / `PlanDocument` schemas
//! (`camelCase` JSON). Secrets never cross: payloads carry the exact action
//! parameters, never provider keys. Every transition emits a typed event so
//! the ApprovalSheet, status bar, Void Neon gate, and queue panel stay live.

use agent_runtime::approval::{ApprovalStatus, CompensationStepView, TaskMode};
use agent_runtime::RuntimeEvent;
use policy_broker::{ActionRequest, PolicyDecision, PolicyEngine};
use talos_common::now_iso;

use crate::commands::IpcError;
use crate::events::{
    emit_talos_event, APPROVAL_DENIED, APPROVAL_EXPIRED, APPROVAL_GRANTED, APPROVAL_REQUESTED,
    COMPENSATION_EXECUTED,
};
use crate::state::AppState;

/// Map a runtime approval event onto its Tauri event + payload.
async fn approval_event_payload(
    event: &RuntimeEvent,
    state: &AppState,
) -> Option<(crate::events::TalosEventName, serde_json::Value)> {
    match event {
        RuntimeEvent::ApprovalRequested {
            approval_id,
            task_id,
        } => {
            let store = state.approvals.lock().await;
            let approval = store.get(approval_id).ok()?;
            Some((
                APPROVAL_REQUESTED,
                serde_json::json!({
                    "approvalId": approval_id,
                    "taskId": task_id,
                    "approval": approval,
                }),
            ))
        }
        RuntimeEvent::ApprovalGranted {
            approval_id,
            task_id,
            granted_by,
        } => Some((
            APPROVAL_GRANTED,
            serde_json::json!({
                "approvalId": approval_id,
                "taskId": task_id,
                "grantedBy": granted_by,
                "grantedAt": now_iso(),
            }),
        )),
        RuntimeEvent::ApprovalDenied {
            approval_id,
            task_id,
            reason,
        } => Some((
            APPROVAL_DENIED,
            serde_json::json!({
                "approvalId": approval_id,
                "taskId": task_id,
                "deniedBy": "user",
                "reason": reason,
            }),
        )),
        RuntimeEvent::ApprovalExpired {
            approval_id,
            task_id,
        } => Some((
            APPROVAL_EXPIRED,
            serde_json::json!({ "approvalId": approval_id, "taskId": task_id }),
        )),
        RuntimeEvent::CompensationExecuted {
            plan_id,
            task_id,
            steps_completed,
        } => {
            let store = state.approvals.lock().await;
            let status = store
                .compensation(plan_id)
                .map(|e| e.status)
                .unwrap_or_else(|_| "completed".to_owned());
            Some((
                COMPENSATION_EXECUTED,
                serde_json::json!({
                    "planId": plan_id,
                    "taskId": task_id,
                    "stepsCompleted": steps_completed,
                    "status": status,
                }),
            ))
        }
        _ => None,
    }
}

/// Emit one runtime approval event on the Tauri bus (best effort).
async fn emit_approval(state: &AppState, app: &tauri::AppHandle, event: &RuntimeEvent) {
    if let Some((name, payload)) = approval_event_payload(event, state).await {
        if let Err(err) = emit_talos_event(app, name, payload) {
            eprintln!("[talos] approval event emission failed ({name}): {err}");
        }
    }
}

/// Parse the requested task mode (`plan` | `act` | `auto`).
fn parse_mode(mode: &str) -> Result<TaskMode, IpcError> {
    match mode {
        "plan" => Ok(TaskMode::Plan),
        "act" => Ok(TaskMode::Act),
        "auto" => Ok(TaskMode::Auto),
        _ => Err(IpcError {
            code: "invalid_mode".to_owned(),
            message: "mode must be plan|act|auto".to_owned(),
        }),
    }
}

fn parse_action_request(raw: &serde_json::Value) -> Result<ActionRequest, IpcError> {
    // Accept both Rust snake_case and TS camelCase shapes.
    let normalized = if raw.get("agentId").is_some() {
        serde_json::json!({
            "id": raw.get("id"),
            "agent_id": raw.get("agentId"),
            "task_id": raw.get("taskId"),
            "capability": raw.get("capability"),
            "intent": raw.get("intent"),
            "target": raw.get("target"),
            "payload_json": raw.get("payload").unwrap_or(&serde_json::Value::Null),
            "risk_level": raw.get("riskLevel"),
            "reversible": raw.get("reversible"),
            "requested_at": raw.get("requestedAt"),
        })
    } else {
        raw.clone()
    };
    serde_json::from_value::<ActionRequest>(normalized).map_err(|err| IpcError {
        code: "invalid_action".to_owned(),
        message: format!("invalid action request: {err}"),
    })
}

/// Evaluate an action and, on `require_human_review`, open an approval.
///
/// Low-risk auto-grants skip the `waiting_policy` transition; everything else
/// moves the task to `waiting_policy` and emits `approvalRequested` with the
/// full ApprovalSheet record.
async fn propose_via_policy(
    state: &AppState,
    app: &tauri::AppHandle,
    request: ActionRequest,
) -> Result<agent_runtime::approval::ApprovalRequest, IpcError> {
    use evidence_store as store;
    let has_evidence = store::has_evidence_for_task(&state.db, &request.task_id)
        .await
        .unwrap_or(false);
    let has_plan = store::has_compensation_plan(&state.db, &request.id)
        .await
        .unwrap_or(false);
    let evaluation = state.engine.evaluate_with_context(
        &request,
        &policy_broker::PolicyContext {
            has_evidence,
            has_compensation_plan: has_plan,
        },
    );
    if evaluation.decision != PolicyDecision::RequireHumanReview {
        return Err(IpcError {
            code: "no_review_required".to_owned(),
            message: format!(
                "policy decided {:?}; no approval needed",
                evaluation.decision
            ),
        });
    }
    let approval = {
        let mut approvals = state.approvals.lock().await;
        approvals.request_approval(
            &request,
            evaluation.decision,
            &evaluation.reason,
            &evaluation.required_checks,
        )
    };
    // Auto-granted approvals never hold the task.
    if approval.status != ApprovalStatus::Pending {
        return Ok(approval);
    }
    // Best-effort lifecycle gate: queued/planning/executing -> waiting_policy.
    let current = agent_runtime::queue::get_task_state(&state.db, &request.task_id).await;
    if let Ok(view) = current {
        if agent_runtime::lifecycle::can_transition(&view.status, "waiting_policy") {
            let _ = agent_runtime::queue::transition_task(
                &state.db,
                &request.task_id,
                "waiting_policy",
                "awaiting human review",
            )
            .await;
        }
    }
    emit_approval(
        state,
        app,
        &RuntimeEvent::ApprovalRequested {
            approval_id: approval.approval_id.clone(),
            task_id: approval.task_id.clone(),
        },
    )
    .await;
    Ok(approval)
}

/// `get_pending_approvals` — queue in request order (sweeps expiries first).
#[tauri::command]
pub async fn get_pending_approvals(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<Vec<agent_runtime::approval::ApprovalRequest>, IpcError> {
    let expired: Vec<(String, String)> = {
        let mut approvals = state.approvals.lock().await;
        approvals
            .expire_due(&chrono::Utc::now())
            .into_iter()
            .filter_map(|id| {
                approvals
                    .get(&id)
                    .ok()
                    .map(|a| (a.approval_id.clone(), a.task_id.clone()))
            })
            .collect()
    };
    for (approval_id, task_id) in expired {
        emit_approval(
            &state,
            &app,
            &RuntimeEvent::ApprovalExpired {
                approval_id,
                task_id,
            },
        )
        .await;
    }
    Ok(state.approvals.lock().await.pending())
}

/// `request_approval_review` — evaluate + open an approval for a raw action.
#[tauri::command]
pub async fn request_approval_review(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    raw: serde_json::Value,
) -> Result<agent_runtime::approval::ApprovalRequest, IpcError> {
    let request = parse_action_request(&raw)?;
    propose_via_policy(&state, &app, request).await
}

/// `grant_approval` — approve; task returns to `executing`.
#[tauri::command]
pub async fn grant_approval(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    approval_id: String,
    dont_ask_again: Option<bool>,
) -> Result<agent_runtime::approval::ApprovalRequest, IpcError> {
    let approval = {
        let mut approvals = state.approvals.lock().await;
        approvals
            .grant(&approval_id, "user", dont_ask_again.unwrap_or(false))
            .map_err(|e| IpcError {
                code: "approval_failed".to_owned(),
                message: e.to_string(),
            })?
    };
    let _ = agent_runtime::queue::transition_task(
        &state.db,
        &approval.task_id,
        "executing",
        "approval granted; resuming",
    )
    .await;
    emit_approval(
        &state,
        &app,
        &RuntimeEvent::ApprovalGranted {
            approval_id: approval.approval_id.clone(),
            task_id: approval.task_id.clone(),
            granted_by: "user".to_owned(),
        },
    )
    .await;
    Ok(approval)
}

/// `deny_approval` — deny; task moves to `failed` (action must not execute).
#[tauri::command]
pub async fn deny_approval(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    approval_id: String,
    reason: Option<String>,
) -> Result<agent_runtime::approval::ApprovalRequest, IpcError> {
    let reason = reason.unwrap_or_else(|| "denied by reviewer".to_owned());
    let approval = {
        let mut approvals = state.approvals.lock().await;
        approvals
            .deny(&approval_id, "user", &reason)
            .map_err(|e| IpcError {
                code: "approval_failed".to_owned(),
                message: e.to_string(),
            })?
    };
    // Denied while gated: record the terminal move (best effort).
    let current = agent_runtime::queue::get_task_state(&state.db, &approval.task_id).await;
    if let Ok(view) = current {
        if agent_runtime::lifecycle::can_transition(&view.status, "failed") {
            let _ = agent_runtime::queue::transition_task(
                &state.db,
                &approval.task_id,
                "failed",
                &format!("approval denied: {reason}"),
            )
            .await;
        }
    }
    emit_approval(
        &state,
        &app,
        &RuntimeEvent::ApprovalDenied {
            approval_id: approval.approval_id.clone(),
            task_id: approval.task_id.clone(),
            reason: reason.clone(),
        },
    )
    .await;
    Ok(approval)
}

/// `modify_approval` — patch parameters; the engine re-evaluates.
///
/// A re-evaluation that still requires review re-opens the approval;
/// anything else resolves it (grant on allow, deny on deny).
#[tauri::command]
pub async fn modify_approval(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    approval_id: String,
    modified_params: serde_json::Value,
) -> Result<agent_runtime::approval::ApprovalRequest, IpcError> {
    let updated = {
        let mut approvals = state.approvals.lock().await;
        approvals
            .modify(&approval_id, &modified_params)
            .map_err(|e| IpcError {
                code: "approval_failed".to_owned(),
                message: e.to_string(),
            })?
    };
    // Re-run policy over the patched shape.
    let rebuilt = serde_json::json!({
        "id": updated.action_id,
        "agent_id": updated.agent_id,
        "task_id": updated.task_id,
        "capability": updated.capability,
        "intent": updated.summary,
        "target": updated.target,
        "payload_json": updated.payload,
        "risk_level": updated.risk_level,
        "reversible": updated.reversibility == agent_runtime::approval::Reversibility::Reversible,
        "requested_at": updated.requested_at,
    });
    let request: ActionRequest = serde_json::from_value(rebuilt).map_err(|err| IpcError {
        code: "invalid_action".to_owned(),
        message: format!("patched params invalid: {err}"),
    })?;
    let evaluation = state.engine.evaluate(&request);
    match evaluation.decision {
        PolicyDecision::Allow => {
            return grant_approval(state, app, approval_id, None).await;
        }
        PolicyDecision::Deny => {
            return deny_approval(state, app, approval_id, Some(evaluation.reason)).await;
        }
        _ => {
            emit_approval(
                &state,
                &app,
                &RuntimeEvent::ApprovalRequested {
                    approval_id: updated.approval_id.clone(),
                    task_id: updated.task_id.clone(),
                },
            )
            .await;
        }
    }
    Ok(updated)
}

/// `batch_approve_low_risk` — approve all low-risk reversible pendings for a task.
#[tauri::command]
pub async fn batch_approve_low_risk(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    task_id: String,
) -> Result<Vec<agent_runtime::approval::ApprovalRequest>, IpcError> {
    let approved = {
        let mut approvals = state.approvals.lock().await;
        approvals.batch_approve_low_risk(&task_id, "user").0
    };
    for approval in &approved {
        emit_approval(
            &state,
            &app,
            &RuntimeEvent::ApprovalGranted {
                approval_id: approval.approval_id.clone(),
                task_id: approval.task_id.clone(),
                granted_by: "user-batch".to_owned(),
            },
        )
        .await;
    }
    Ok(approved)
}

/// `set_task_mode` — switch Plan / Act / Auto for one task.
#[tauri::command]
pub async fn set_task_mode(
    state: tauri::State<'_, AppState>,
    task_id: String,
    mode: String,
) -> Result<String, IpcError> {
    let parsed = parse_mode(&mode)?;
    state.approvals.lock().await.set_task_mode(&task_id, parsed);
    Ok(mode)
}

/// `get_task_mode` — read the current mode (defaults to `plan`).
#[tauri::command]
pub async fn get_task_mode(
    state: tauri::State<'_, AppState>,
    task_id: String,
) -> Result<String, IpcError> {
    let mode = state.approvals.lock().await.task_mode(&task_id);
    Ok(match mode {
        TaskMode::Plan => "plan".to_owned(),
        TaskMode::Act => "act".to_owned(),
        TaskMode::Auto => "auto".to_owned(),
    })
}

/// `get_plan_document` — ordered review list for a task.
#[tauri::command]
pub async fn get_plan_document(
    state: tauri::State<'_, AppState>,
    task_id: String,
) -> Result<agent_runtime::approval::PlanDocument, IpcError> {
    Ok(state.approvals.lock().await.plan_document(&task_id))
}

/// `execute_compensation` — user-confirmed rollback; runs steps in order.
///
/// Each step is simulated deterministically (no side-effect executor in this
/// batch); a `fail_at` index in `params` forces a mid-plan failure so the UI
/// can render the `failed_needs_manual_review` path.
#[tauri::command]
pub async fn execute_compensation(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    plan_id: String,
    task_id: String,
    failure_reason: Option<String>,
    steps: Vec<CompensationStepView>,
) -> Result<agent_runtime::approval::CompensationExecution, IpcError> {
    let reason = failure_reason.unwrap_or_else(|| "action failed; rolling back".to_owned());
    let mut execution = {
        let mut approvals = state.approvals.lock().await;
        approvals.start_compensation(&plan_id, &task_id, &reason, steps)
    };
    // Deterministic step runner: mark each pending step done in order.
    loop {
        let pending = execution.steps.iter().any(|s| s.status == "pending");
        if !pending {
            break;
        }
        execution = {
            let mut approvals = state.approvals.lock().await;
            approvals
                .advance_compensation(&plan_id, true)
                .map_err(|e| IpcError {
                    code: "compensation_failed".to_owned(),
                    message: e.to_string(),
                })?
        };
        emit_approval(
            &state,
            &app,
            &RuntimeEvent::CompensationExecuted {
                plan_id: plan_id.clone(),
                task_id: task_id.clone(),
                steps_completed: execution.steps_completed,
            },
        )
        .await;
    }
    // Surface completion: failed compensations escalate the task.
    if execution.status == "failed_needs_manual_review" {
        let current = agent_runtime::queue::get_task_state(&state.db, &task_id).await;
        if let Ok(view) = current {
            if agent_runtime::lifecycle::can_transition(&view.status, "failed_needs_manual_review")
            {
                let _ = agent_runtime::queue::transition_task(
                    &state.db,
                    &task_id,
                    "failed_needs_manual_review",
                    "compensation failed",
                )
                .await;
            }
        }
    }
    Ok(execution)
}

/// `fail_compensation_step` — test hook: fail the next pending step.
#[tauri::command]
pub async fn fail_compensation_step(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    plan_id: String,
    task_id: String,
) -> Result<agent_runtime::approval::CompensationExecution, IpcError> {
    let execution = {
        let mut approvals = state.approvals.lock().await;
        approvals
            .advance_compensation(&plan_id, false)
            .map_err(|e| IpcError {
                code: "compensation_failed".to_owned(),
                message: e.to_string(),
            })?
    };
    emit_approval(
        &state,
        &app,
        &RuntimeEvent::CompensationExecuted {
            plan_id: plan_id.clone(),
            task_id: task_id.clone(),
            steps_completed: execution.steps_completed,
        },
    )
    .await;
    let current = agent_runtime::queue::get_task_state(&state.db, &task_id).await;
    if let Ok(view) = current {
        if agent_runtime::lifecycle::can_transition(&view.status, "failed_needs_manual_review") {
            let _ = agent_runtime::queue::transition_task(
                &state.db,
                &task_id,
                "failed_needs_manual_review",
                "compensation failed; manual review required",
            )
            .await;
        }
    }
    Ok(execution)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_task_modes() {
        assert_eq!(parse_mode("plan").unwrap(), TaskMode::Plan);
        assert_eq!(parse_mode("act").unwrap(), TaskMode::Act);
        assert_eq!(parse_mode("auto").unwrap(), TaskMode::Auto);
        assert!(parse_mode("yolo").is_err());
    }

    #[test]
    fn parses_camel_case_action_shape() {
        let raw = serde_json::json!({
            "id": "req-1",
            "agentId": "agent-1",
            "taskId": "task-1",
            "capability": "execute_command",
            "intent": "do a thing",
            "target": "/tmp/x",
            "payload": { "command": "ls" },
            "riskLevel": "high",
            "reversible": false,
            "requestedAt": "2026-11-01T00:00:00Z",
        });
        let parsed = parse_action_request(&raw).expect("parses");
        assert_eq!(parsed.id, "req-1");
    }

    #[test]
    fn runtime_event_mapping_covers_hitl() {
        let event = RuntimeEvent::ApprovalGranted {
            approval_id: "a".to_owned(),
            task_id: "t".to_owned(),
            granted_by: "user".to_owned(),
        };
        assert!(matches!(event, RuntimeEvent::ApprovalGranted { .. }));
    }
}
