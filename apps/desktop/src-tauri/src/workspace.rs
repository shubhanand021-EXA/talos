//! Coding workspace IPC: per-task isolated roots for CoderAgent work.
//!
//! Workspaces isolate each coding task: a git worktree when the task
//! references a repo, otherwise an isolated temp directory. Tool execution
//! for workspace tasks resolves the sandbox root to the workspace root, with
//! the coder allowlist and network-gated installs; every other task keeps the
//! shared tool root with execution disabled.

use talos_common::TalosError;

use crate::commands::IpcError;
use crate::events::{emit_talos_event, WORKSPACE_UPDATED};
use crate::state::AppState;

/// Workspace info shaped for the TS `WorkspaceInfo` schema (`camelCase`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceInfo {
    /// Owning task.
    pub task_id: String,
    /// Isolated root path.
    pub root: String,
    /// Workspace branch (`talos/<task>` when git-backed).
    pub branch: String,
    /// True when created via `git worktree add`.
    pub is_git_worktree: bool,
}

impl From<workspace_manager::Workspace> for WorkspaceInfo {
    fn from(workspace: workspace_manager::Workspace) -> Self {
        Self {
            task_id: workspace.task_id,
            root: workspace.root.to_string_lossy().into_owned(),
            branch: workspace.branch,
            is_git_worktree: workspace.is_git_worktree,
        }
    }
}

fn emit_workspace(app: Option<&tauri::AppHandle>, info: &WorkspaceInfo, status: &str) {
    let Some(app) = app else {
        return;
    };
    let payload = serde_json::json!({
        "taskId": info.task_id,
        "path": info.root,
        "status": status,
        "checkedAt": talos_common::now_iso(),
    });
    if let Err(err) = emit_talos_event(app, WORKSPACE_UPDATED, payload) {
        eprintln!("[talos] event emission failed ({WORKSPACE_UPDATED}): {err}");
    }
}

fn check_task_id(task_id: &str) -> Result<(), IpcError> {
    if task_id.trim().is_empty() || task_id.len() > 128 {
        return Err(IpcError::from(TalosError::Storage(
            "task_id must be 1..=128 chars".to_owned(),
        )));
    }
    Ok(())
}

/// Inner create (shared by the command and tests; emits when `app` given).
pub async fn create_workspace_inner(
    state: &AppState,
    app: Option<&tauri::AppHandle>,
    task_id: String,
    repo_path: Option<String>,
) -> Result<WorkspaceInfo, IpcError> {
    check_task_id(&task_id)?;
    // Tasks must exist before they get a workspace (ledger linkage).
    agent_runtime::queue::get_task_state(&state.db, &task_id).await?;
    let repo = repo_path
        .filter(|path| !path.trim().is_empty())
        .map(std::path::PathBuf::from);
    let workspace = {
        let workspaces = state.workspaces.lock().await;
        workspaces
            .create_workspace(&task_id, repo.as_deref())
            .await?
    };
    let info = WorkspaceInfo::from(workspace);
    emit_workspace(app, &info, "created");
    Ok(info)
}

/// Inner cleanup (shared by the command and tests).
pub async fn cleanup_workspace_inner(
    state: &AppState,
    app: Option<&tauri::AppHandle>,
    task_id: String,
) -> Result<bool, IpcError> {
    check_task_id(&task_id)?;
    let removed = {
        let workspaces = state.workspaces.lock().await;
        workspaces.cleanup(&task_id).await?
    };
    let root = {
        let workspaces = state.workspaces.lock().await;
        workspaces
            .workspace_root(&task_id)
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    emit_workspace(
        app,
        &WorkspaceInfo {
            task_id,
            root,
            branch: String::new(),
            is_git_worktree: false,
        },
        "cleaned",
    );
    Ok(removed)
}

/// `create_workspace` — isolate a root for `task_id` (idempotent).
#[tauri::command]
pub async fn create_workspace(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    task_id: String,
    repo_path: Option<String>,
) -> Result<WorkspaceInfo, IpcError> {
    create_workspace_inner(&state, Some(&app), task_id, repo_path).await
}

/// `get_workspace` — workspace info when the dir exists, else transport error.
#[tauri::command]
pub async fn get_workspace(
    state: tauri::State<'_, AppState>,
    task_id: String,
) -> Result<WorkspaceInfo, IpcError> {
    check_task_id(&task_id)?;
    let workspaces = state.workspaces.lock().await;
    workspaces
        .get_workspace(&task_id)
        .await?
        .map(WorkspaceInfo::from)
        .ok_or_else(|| {
            IpcError::from(TalosError::Storage(format!(
                "no workspace for task {task_id}"
            )))
        })
}

/// `list_workspaces` — all existing task workspaces, sorted by task id.
#[tauri::command]
pub async fn list_workspaces(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<WorkspaceInfo>, IpcError> {
    let workspaces = state.workspaces.lock().await;
    Ok(workspaces
        .list_workspaces()
        .await
        .into_iter()
        .map(WorkspaceInfo::from)
        .collect())
}

/// `cleanup_workspace` — remove the worktree registration + dir (idempotent).
#[tauri::command]
pub async fn cleanup_workspace(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    task_id: String,
) -> Result<bool, IpcError> {
    cleanup_workspace_inner(&state, Some(&app), task_id).await
}

/// `get_workspace_tree` — bounded `fs.list` of the workspace root for the UI
/// file tree. Goes through the policy-gated executor (Low read: allowed
/// without prior evidence) so every listing is audited like any other tool.
#[tauri::command]
pub async fn get_workspace_tree(
    state: tauri::State<'_, AppState>,
    task_id: String,
    depth: Option<u8>,
) -> Result<serde_json::Value, IpcError> {
    check_task_id(&task_id)?;
    let root = {
        let workspaces = state.workspaces.lock().await;
        workspaces
            .workspace_root(&task_id)
            .map_err(IpcError::from)?
    };
    let config = tool_runtime::SandboxConfig {
        root,
        ..tool_runtime::SandboxConfig::default()
    };
    let depth = depth.unwrap_or(2).clamp(0, 4);
    let registry = state.tools.lock().await;
    let outcome = tool_runtime::invoke(
        &state.db,
        &state.engine,
        &registry,
        &config,
        tool_runtime::ToolInvocation {
            tool_id: tool_runtime::FS_LIST.to_owned(),
            task_id: task_id.clone(),
            agent_id: "workspace-service".to_owned(),
            params: serde_json::json!({ "path": ".", "depth": depth }),
            dry_run: false,
        },
    )
    .await?;
    if outcome.receipt.status != tool_runtime::ToolStatus::Success {
        return Err(IpcError::from(TalosError::Storage(
            outcome
                .receipt
                .error
                .map(|e| e.message)
                .unwrap_or_else(|| "workspace tree unavailable".to_owned()),
        )));
    }
    Ok(outcome.receipt.output)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_state() -> AppState {
        AppState::new_in_memory()
            .await
            .expect("in-memory state builds")
    }

    async fn submit_task(state: &AppState, title: &str) -> agent_runtime::queue::TaskView {
        let seq = {
            let mut runtime = state.runtime.lock().await;
            runtime.next_queue_seq()
        };
        let mut holder = seq;
        agent_runtime::queue::submit_task(
            &state.db,
            &mut holder,
            agent_runtime::queue::SubmitParams {
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
    async fn workspace_lifecycle_round_trip() {
        let state = test_state().await;
        let task = submit_task(&state, "code the widget").await;
        let created = create_workspace_inner(&state, None, task.id.clone(), None)
            .await
            .expect("create");
        assert!(std::path::Path::new(&created.root).exists());
        // Idempotent re-create reuses the dir.
        let again = create_workspace_inner(&state, None, task.id.clone(), None)
            .await
            .expect("reuse");
        assert_eq!(created.root, again.root);
        // Tree lists the workspace root through the gated executor.
        // (Locks sequenced workspaces-then-tools to match invoke order.)
        let root = {
            let workspaces = state.workspaces.lock().await;
            workspaces.workspace_root(&task.id).expect("root")
        };
        let tree = {
            let config = tool_runtime::SandboxConfig {
                root,
                ..tool_runtime::SandboxConfig::default()
            };
            let registry = state.tools.lock().await;
            tool_runtime::invoke(
                &state.db,
                &state.engine,
                &registry,
                &config,
                tool_runtime::ToolInvocation {
                    tool_id: tool_runtime::FS_LIST.to_owned(),
                    task_id: task.id.clone(),
                    agent_id: "workspace-service".to_owned(),
                    params: serde_json::json!({ "path": ".", "depth": 1 }),
                    dry_run: false,
                },
            )
            .await
            .expect("tree")
            .receipt
            .output
        };
        assert!(tree.get("entries").is_some());
        assert!(cleanup_workspace_inner(&state, None, task.id.clone())
            .await
            .expect("cleanup"));
        assert!(!std::path::Path::new(&created.root).exists());
    }

    #[tokio::test]
    async fn workspace_requires_existing_task() {
        let state = test_state().await;
        let missing = create_workspace_inner(&state, None, "task-missing".to_owned(), None).await;
        assert!(missing.is_err());
    }

    #[tokio::test]
    async fn workspace_rejects_bad_ids() {
        let state = test_state().await;
        assert!(create_workspace_inner(&state, None, String::new(), None)
            .await
            .is_err());
        assert!(
            create_workspace_inner(&state, None, "../escape".to_owned(), None)
                .await
                .is_err()
        );
    }
}
