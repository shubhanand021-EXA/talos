//! OS sandbox IPC: platform status, per-task profiles, danger approval gate.
//!
//! The desktop resolves the [`sandbox::SandboxProfile`] for every tool
//! execution: workspace-backed tasks run `workspace_write`, everything else
//! `read_only`, and `danger_full_access` only with an explicit human approval
//! (held in the HITL [`agent_runtime::approval::ApprovalStore`]).
//! Enforcement itself lives in `tool_runtime` + the `sandbox` crate — this
//! layer only routes, approves, and reports. Every denial fails closed.

use agent_runtime::approval::{ApprovalRequest, ApprovalStatus};
use talos_common::TalosError;

use crate::commands::IpcError;
use crate::events::{emit_talos_event, APPROVAL_REQUESTED, SANDBOX_ENFORCED, SANDBOX_VIOLATION};
use crate::state::AppState;

/// Marker placed in danger-approval summaries so grants are matchable.
pub const DANGER_APPROVAL_MARKER: &str = "DANGER full-access execution";

/// Backend status for the UI (`get_sandbox_status`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxBackendView {
    /// Backend name (`bubblewrap`, `seatbelt`, `windows-job-objects`).
    pub name: String,
    /// Whether it can confine a child on this machine.
    pub available: bool,
    /// Enforcement description (or the missing piece + limitations).
    pub enforcement_method: String,
    /// Availability detail (versions, limitations).
    pub detail: String,
}

/// Platform sandbox status (`get_sandbox_status`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxStatusView {
    /// OS platform (`linux`, `macos`, `windows`, …).
    pub platform: String,
    /// Always true: Talos denies rather than running unsandboxed.
    pub fail_closed: bool,
    /// Per-backend status.
    pub backends: Vec<SandboxBackendView>,
    /// RFC 3339 check time.
    pub checked_at: String,
}

/// Profile resolution for one task (`get_sandbox_profile_for_task`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxProfileForTaskView {
    /// Task the profile was resolved for.
    pub task_id: String,
    /// Resolved profile.
    pub profile: String,
    /// Whether an OS backend would enforce it (read-only denial counts).
    pub enforced: bool,
    /// Enforcement description.
    pub enforcement_method: String,
    /// Why this profile was chosen.
    pub reason: String,
}

/// `get_sandbox_status` — platform, backends, fail-closed flag.
#[tauri::command]
pub async fn get_sandbox_status() -> Result<SandboxStatusView, IpcError> {
    let status = sandbox::sandbox_status();
    Ok(SandboxStatusView {
        platform: status.platform,
        fail_closed: status.fail_closed,
        backends: status
            .backends
            .into_iter()
            .map(|backend| SandboxBackendView {
                name: backend.name,
                available: backend.available,
                enforcement_method: backend.enforcement_method,
                detail: backend.detail,
            })
            .collect(),
        checked_at: status.checked_at,
    })
}

/// `get_sandbox_profile_for_task` — resolve the profile without executing.
#[tauri::command]
pub async fn get_sandbox_profile_for_task(
    state: tauri::State<'_, AppState>,
    task_id: String,
) -> Result<SandboxProfileForTaskView, IpcError> {
    if task_id.trim().is_empty() || task_id.len() > 128 {
        return Err(IpcError::from(TalosError::Storage(
            "task_id must be 1..=128 chars".to_owned(),
        )));
    }
    Ok(resolve_profile_for_task(&state, &task_id).await)
}

/// Resolve the sandbox profile for a task (no approval side effects).
pub async fn resolve_profile_for_task(
    state: &AppState,
    task_id: &str,
) -> SandboxProfileForTaskView {
    let in_workspace = {
        let workspaces = state.workspaces.lock().await;
        workspaces.exists(task_id).await
    };
    if in_workspace {
        let enforcer = sandbox::select_enforcer();
        let enforced = enforcer.is_available();
        let root = {
            let workspaces = state.workspaces.lock().await;
            workspaces
                .workspace_root(task_id)
                .unwrap_or_else(|_| state.tool_root.clone())
        };
        let probe = sandbox::SandboxConfig {
            workspace_root: root,
            network_allowed: false,
            ..sandbox::SandboxConfig::default()
        };
        SandboxProfileForTaskView {
            task_id: task_id.to_owned(),
            profile: sandbox::SandboxProfile::WorkspaceWrite.as_str().to_owned(),
            enforced,
            enforcement_method: if enforced {
                enforcer.enforcement_method(&probe)
            } else {
                "unavailable — execution denied (fail closed)".to_owned()
            },
            reason: "workspace-backed task executes confined".to_owned(),
        }
    } else {
        SandboxProfileForTaskView {
            task_id: task_id.to_owned(),
            profile: sandbox::SandboxProfile::ReadOnly.as_str().to_owned(),
            enforced: true,
            enforcement_method: "deny-exec(read-only)".to_owned(),
            reason: "no workspace: process execution is denied".to_owned(),
        }
    }
}

/// Granted danger approval for a task, if any (explicit human grant only).
async fn granted_danger_approval(state: &AppState, task_id: &str) -> Option<ApprovalRequest> {
    let approvals = state.approvals.lock().await;
    approvals.all().into_iter().find(|approval| {
        approval.task_id == task_id
            && approval.status == ApprovalStatus::Granted
            && approval.summary.contains(DANGER_APPROVAL_MARKER)
    })
}

/// Hold `danger_full_access` for explicit human approval.
///
/// Returns the approval id when already granted; otherwise records a pending
/// approval, emits `approvalRequested`, and errors with `approval_required`
/// (the UI opens the ApprovalSheet; the caller retries after the grant).
pub async fn ensure_danger_approval(
    state: &AppState,
    app: Option<&tauri::AppHandle>,
    task_id: &str,
    agent_id: &str,
    tool_id: &str,
    params: &serde_json::Value,
) -> Result<String, IpcError> {
    if let Some(approval) = granted_danger_approval(state, task_id).await {
        return Ok(approval.approval_id);
    }
    let request = policy_broker::ActionRequest {
        id: format!("danger-{task_id}-{tool_id}"),
        agent_id: agent_id.to_owned(),
        task_id: task_id.to_owned(),
        capability: policy_broker::Capability::ExecuteCommand,
        intent: format!("{DANGER_APPROVAL_MARKER} via {tool_id}"),
        target: tool_id.to_owned(),
        payload_json: params.clone(),
        risk_level: policy_broker::RiskLevel::High,
        reversible: false,
        requested_at: chrono::Utc::now(),
    };
    let approval = {
        let mut approvals = state.approvals.lock().await;
        approvals.request_approval(
            &request,
            policy_broker::PolicyDecision::RequireHumanReview,
            "danger_full_access runs with full user access and no OS sandbox; explicit human approval required",
            &[],
        )
    };
    let approval_id = approval.approval_id.clone();
    if let Some(app) = app {
        let _ = emit_talos_event(
            app,
            APPROVAL_REQUESTED,
            serde_json::json!({
                "approvalId": approval.approval_id,
                "taskId": task_id,
                "approval": approval,
            }),
        );
    }
    Err(IpcError {
        code: "approval_required".to_owned(),
        message: format!(
            "danger_full_access requires human approval (approval_id={approval_id}); approve in the ApprovalSheet, then retry"
        ),
    })
}

/// Emit `sandboxEnforced` / `sandboxViolation` from a tool receipt.
///
/// Reads `receipt.output.sandbox` (receipt JSON) and `receipt.error`: an
/// enforced run emits `sandboxEnforced`; runtime violations or a
/// `sandbox_unavailable` denial emit `sandboxViolation`.
pub async fn emit_sandbox_outcome(
    app: &tauri::AppHandle,
    tool_id: &str,
    receipt: &tool_runtime::ToolReceipt,
) {
    let task_id = receipt.task_id.clone();
    let sandbox = receipt.output.get("sandbox");
    let enforced = sandbox
        .and_then(|value| value.get("enforced"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let profile = sandbox
        .and_then(|value| value.get("profile"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("workspace_write")
        .to_owned();
    let method = sandbox
        .and_then(|value| value.get("enforcementMethod"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown")
        .to_owned();
    if enforced {
        let _ = emit_talos_event(
            app,
            SANDBOX_ENFORCED,
            serde_json::json!({
                "taskId": task_id,
                "toolId": tool_id,
                "profile": profile,
                "enforcementMethod": method,
                "checkedAt": talos_common::now_iso(),
            }),
        );
    }
    let violations: Vec<String> = sandbox
        .and_then(|value| value.get("violations"))
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let denied_unavailable = receipt
        .error
        .as_ref()
        .is_some_and(|error| error.code == sandbox::SANDBOX_UNAVAILABLE);
    if denied_unavailable || !violations.is_empty() {
        let detail = if denied_unavailable {
            receipt
                .error
                .as_ref()
                .map(|error| error.message.clone())
                .unwrap_or_else(|| "sandbox unavailable; execution denied".to_owned())
        } else {
            violations.join("; ")
        };
        let _ = emit_talos_event(
            app,
            SANDBOX_VIOLATION,
            serde_json::json!({
                "taskId": task_id,
                "toolId": tool_id,
                "profile": profile,
                "code": if denied_unavailable { sandbox::SANDBOX_UNAVAILABLE } else { "sandbox_violation" },
                "detail": detail,
                "checkedAt": talos_common::now_iso(),
            }),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_state() -> AppState {
        AppState::new_in_memory()
            .await
            .expect("in-memory state builds")
    }

    #[tokio::test]
    async fn status_reports_fail_closed() {
        let status = get_sandbox_status().await.expect("status");
        assert!(status.fail_closed);
        assert!(!status.platform.is_empty());
        assert!(!status.backends.is_empty());
        assert!(!status.checked_at.is_empty());
    }

    #[tokio::test]
    async fn non_workspace_task_resolves_read_only() {
        let state = test_state().await;
        let view = resolve_profile_for_task(&state, "task-no-workspace").await;
        assert_eq!(view.profile, "read_only");
        assert!(view.enforced);
    }

    #[tokio::test]
    async fn danger_requires_approval_then_grants() {
        let state = test_state().await;
        let task_id = "task-danger-1";
        let first = ensure_danger_approval(
            &state,
            None,
            task_id,
            "agent-1",
            "process.exec",
            &serde_json::json!({}),
        )
        .await;
        let err = first.expect_err("no grant yet");
        assert_eq!(err.code, "approval_required");
        let pending = state.approvals.lock().await.pending();
        let approval = pending
            .iter()
            .find(|a| a.task_id == task_id)
            .expect("pending danger approval");
        let approval_id = approval.approval_id.clone();
        state
            .approvals
            .lock()
            .await
            .grant(&approval_id, "user", false)
            .expect("grant");
        let second = ensure_danger_approval(
            &state,
            None,
            task_id,
            "agent-1",
            "process.exec",
            &serde_json::json!({}),
        )
        .await
        .expect("granted danger proceeds");
        assert_eq!(second, approval_id);
    }
}
