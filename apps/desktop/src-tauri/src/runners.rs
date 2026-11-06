//! BYOC runner management: `.talos/runners.json` registry, per-task
//! assignment, and connection tests.
//!
//! Secrets policy: runner files hold key *paths*, never key material —
//! private keys stay `0600` user files read by the local `ssh` client.
//! The daemon never sees tokens here; only paths cross IPC.

use std::sync::Arc;

use talos_common::{Result, TalosError};

use crate::state::AppState;

/// Read model for the UI Runners tab (never carries key material).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunnerStatusView {
    /// Runner id.
    pub id: String,
    /// Transport (`ssh` | `docker`).
    pub kind: String,
    /// Whether tasks may target it.
    pub enabled: bool,
    /// Last connection test outcome, if any.
    pub last_test_ok: Option<bool>,
    /// Last connection test message, if any.
    pub last_test_message: Option<String>,
}

/// Resolve the runners file (`<app_data>/runners.json`).
fn runners_path(state: &AppState) -> Result<std::path::PathBuf> {
    state
        .app_data_dir
        .clone()
        .map(|base| base.join(remote_runner::config::RUNNERS_FILE_NAME))
        .ok_or_else(|| TalosError::Storage("runners unavailable in-memory".to_owned()))
}

/// Load the registry file (missing ⇒ empty).
pub fn load_runners(state: &AppState) -> Result<remote_runner::config::RunnerRegistryFile> {
    let path = runners_path(state)?;
    let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    else {
        return Ok(remote_runner::config::RunnerRegistryFile::default());
    };
    Ok(remote_runner::config::RunnerRegistryFile::load_or_default(
        parent,
    ))
}

/// Persist the registry file.
pub fn save_runners(
    state: &AppState,
    file: &remote_runner::config::RunnerRegistryFile,
) -> Result<()> {
    let path = runners_path(state)?;
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .map_err(|err| TalosError::Storage(format!("create runners dir: {err}")))?;
    }
    let text = serde_json::to_string_pretty(file)
        .map_err(|err| TalosError::Storage(format!("runners serialize: {err}")))?;
    std::fs::write(&path, text)
        .map_err(|err| TalosError::Storage(format!("write {}: {err}", path.display())))?;
    Ok(())
}

/// Status rows for the UI (enabled + last test outcome).
pub fn status_rows(state: &AppState) -> Result<Vec<RunnerStatusView>> {
    let file = load_runners(state)?;
    Ok(file
        .runners
        .iter()
        .map(|runner| {
            let kind = match &runner.kind {
                remote_runner::config::RunnerKind::Ssh(_) => "ssh",
                remote_runner::config::RunnerKind::Docker(_) => "docker",
            }
            .to_owned();
            RunnerStatusView {
                id: runner.id.clone(),
                kind,
                enabled: runner.enabled,
                last_test_ok: None,
                last_test_message: None,
            }
        })
        .collect())
}

/// Add or replace one runner (validated; secret-free).
pub fn store_runner(
    state: &AppState,
    runner: remote_runner::config::RunnerConfig,
) -> Result<Vec<RunnerStatusView>> {
    let mut file = load_runners(state)?;
    file.runners.retain(|existing| existing.id != runner.id);
    file.runners.push(runner);
    file.validate()?;
    save_runners(state, &file)?;
    status_rows(state)
}

/// Remove one runner (idempotent) and clear its task assignments.
pub async fn delete_runner(state: &AppState, id: &str) -> Result<Vec<RunnerStatusView>> {
    let mut file = load_runners(state)?;
    file.runners.retain(|runner| runner.id != id);
    save_runners(state, &file)?;
    state
        .task_runners
        .lock()
        .await
        .retain(|_, assigned| assigned != id);
    status_rows(state)
}

/// Resolve one enabled runner into an executable target.
pub fn resolve_runner(state: &AppState, id: &str) -> Result<remote_runner::RemoteRunner> {
    let file = load_runners(state)?;
    let config = file
        .find_enabled(id)
        .cloned()
        .ok_or_else(|| TalosError::Storage(format!("unknown or disabled runner: {id}")))?;
    // Fail closed when an SSH key path is missing (fail at assignment/test
    // time, never mid-execution).
    if let remote_runner::config::RunnerKind::Ssh(ssh) = &config.kind {
        let expanded = remote_runner::config::expand_home(&ssh.key_path);
        if !std::path::Path::new(&expanded).is_file() {
            return Err(TalosError::Storage(format!(
                "runner '{id}': key file missing: {}",
                ssh.key_path
            )));
        }
    }
    Ok(remote_runner::RemoteRunner::new(
        config,
        remote_runner::SystemRunner::shared(),
    ))
}

/// Test one runner: `ssh ... true` or `docker ... info` with a 15s cap.
/// Never throws transport details beyond the bounded message.
pub async fn probe_runner(state: &AppState, id: &str) -> Result<RunnerStatusView> {
    let file = load_runners(state)?;
    let config = file
        .runners
        .iter()
        .find(|runner| runner.id == id)
        .cloned()
        .ok_or_else(|| TalosError::Storage(format!("unknown runner: {id}")))?;
    let commands = remote_runner::SystemRunner::shared();
    let timeout = std::time::Duration::from_secs(15);
    let outcome: std::result::Result<remote_runner::CommandOutput, remote_runner::RunnerError> =
        match &config.kind {
            remote_runner::config::RunnerKind::Ssh(ssh) => {
                let target = remote_runner::SshTarget::new(ssh.clone(), Arc::clone(&commands));
                let (program, args) = target.ssh_argv(&["true".to_owned()]);
                commands.run(&program, &args, None, timeout).await
            }
            remote_runner::config::RunnerKind::Docker(docker) => {
                let mut argv = Vec::new();
                if let Some(context) = docker.context.as_deref() {
                    argv.extend(["--context".to_owned(), context.to_owned()]);
                }
                argv.extend([
                    "info".to_owned(),
                    "--format".to_owned(),
                    "{{json .}}".to_owned(),
                ]);
                commands.run("docker", &argv, None, timeout).await
            }
        };
    let (ok, message) = match outcome {
        Ok(output) if output.status == Some(0) => (true, "reachable".to_owned()),
        Ok(output) => (
            false,
            format!(
                "exit {}: {}",
                output.status.unwrap_or(-1),
                String::from_utf8_lossy(&output.stderr)
                    .chars()
                    .take(256)
                    .collect::<String>()
            ),
        ),
        Err(err) => (false, err.to_string().chars().take(256).collect()),
    };
    let kind = match &config.kind {
        remote_runner::config::RunnerKind::Ssh(_) => "ssh",
        remote_runner::config::RunnerKind::Docker(_) => "docker",
    }
    .to_owned();
    Ok(RunnerStatusView {
        id: config.id,
        kind,
        enabled: config.enabled,
        last_test_ok: Some(ok),
        last_test_message: Some(message),
    })
}

/// Assign (or clear with `None`) a task's BYOC runner. Resolution happens at
/// assignment time so `invoke_tool` never fails mid-flight on a bad id.
pub async fn assign_task_runner(
    state: &AppState,
    task_id: &str,
    runner_id: Option<String>,
) -> Result<Option<String>> {
    if task_id.trim().is_empty() || task_id.len() > 128 {
        return Err(TalosError::Storage(
            "task_id must be 1..=128 chars".to_owned(),
        ));
    }
    match runner_id {
        Some(id) => {
            resolve_runner(state, &id)?;
            state
                .task_runners
                .lock()
                .await
                .insert(task_id.to_owned(), id.clone());
            Ok(Some(id))
        }
        None => {
            state.task_runners.lock().await.remove(task_id);
            Ok(None)
        }
    }
}

/// Current assignment for one task, if any.
pub async fn assigned_task_runner(state: &AppState, task_id: &str) -> Option<String> {
    state.task_runners.lock().await.get(task_id).cloned()
}

/// Structured IPC error for runner commands.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RunnerIpcError {
    /// Stable error code.
    pub code: String,
    /// Human-readable detail.
    pub message: String,
}

impl From<TalosError> for RunnerIpcError {
    fn from(err: TalosError) -> Self {
        let (code, message) = match &err {
            TalosError::InvalidId { .. } => ("invalid_id", err.to_string()),
            TalosError::Serialization(_) => ("serialization", err.to_string()),
            TalosError::Storage(message) => ("storage_unavailable", message.clone()),
            TalosError::Policy(_) => ("policy_failure", err.to_string()),
            TalosError::Time(_) => ("time_failure", err.to_string()),
            TalosError::Internal(message) => ("internal", message.clone()),
        };
        Self {
            code: code.to_owned(),
            message,
        }
    }
}

/// `list_runners` — runner registry status (never key material).
#[tauri::command]
pub async fn list_runners(
    state: tauri::State<'_, AppState>,
) -> std::result::Result<Vec<RunnerStatusView>, RunnerIpcError> {
    status_rows(&state).map_err(RunnerIpcError::from)
}

/// `add_runner` — validate + persist one runner definition.
#[tauri::command]
pub async fn add_runner(
    state: tauri::State<'_, AppState>,
    runner: remote_runner::config::RunnerConfig,
) -> std::result::Result<Vec<RunnerStatusView>, RunnerIpcError> {
    store_runner(&state, runner).map_err(RunnerIpcError::from)
}

/// `remove_runner` — delete one runner + clear its task assignments.
#[tauri::command]
pub async fn remove_runner(
    state: tauri::State<'_, AppState>,
    runner_id: String,
) -> std::result::Result<Vec<RunnerStatusView>, RunnerIpcError> {
    delete_runner(&state, &runner_id)
        .await
        .map_err(RunnerIpcError::from)
}

/// `test_runner` — live connectivity probe (`ssh ... true` / `docker info`).
#[tauri::command]
pub async fn test_runner(
    state: tauri::State<'_, AppState>,
    runner_id: String,
) -> std::result::Result<RunnerStatusView, RunnerIpcError> {
    probe_runner(&state, &runner_id)
        .await
        .map_err(RunnerIpcError::from)
}

/// `set_task_runner` — assign (or clear with `null`) a task's BYOC runner.
#[tauri::command]
pub async fn set_task_runner(
    state: tauri::State<'_, AppState>,
    task_id: String,
    runner_id: Option<String>,
) -> std::result::Result<Option<String>, RunnerIpcError> {
    assign_task_runner(&state, &task_id, runner_id)
        .await
        .map_err(RunnerIpcError::from)
}

/// `get_task_runner` — current assignment for one task, if any.
#[tauri::command]
pub async fn get_task_runner(
    state: tauri::State<'_, AppState>,
    task_id: String,
) -> std::result::Result<Option<String>, RunnerIpcError> {
    Ok(assigned_task_runner(&state, &task_id).await)
}
