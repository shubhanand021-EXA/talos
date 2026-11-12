//! Ephemeral-VM computer backend: desktop IPC surface.
//!
//! The agent NEVER touches the host OS. Live computer actions execute inside
//! a per-task Linux desktop container managed by [`computer_use::vm::VmManager`]
//! (process-global; shared by the tool executor and these commands):
//!
//! - `computer_vm_launch` — spawn (or reuse) the task container with a
//!   read-only workspace mount. Params: `taskId`, optional `vncExpose`,
//!   `width`, `height`, `image`.
//! - `computer_vm_status` — running container, loopback VNC URL, resolution.
//! - `computer_vm_destroy` — force-remove the container (idempotent).
//! - `computer_vm_frame` — current guest frame as a `VncStreamEvent`
//!   (`pngBase64` data + hash + seq; polled by the UI viewer at ~1fps).
//! - `computer_vm_pause` / `computer_vm_resume` — freeze/unfreeze the guest
//!   (emergency-stop path).
//!
//! Frames are metadata + bounded PNG data URIs; nothing raw or secret
//! crosses IPC (the guest holds no secrets by construction).

use computer_use::vm::{VmConfig, VmFrame, VmManager, VmSessionView};
use talos_common::TalosError;

use crate::commands::IpcError;
use crate::state::AppState;

/// Resolve the VM manager (process-global production instance).
fn manager() -> std::sync::Arc<VmManager> {
    VmManager::global()
}

/// Build a session config from IPC params (validated, bounded).
fn config_from_params(params: &serde_json::Value) -> Result<VmConfig, IpcError> {
    let mut config = VmConfig::default();
    if let Some(image) = params.get("image").and_then(|value| value.as_str()) {
        if image.trim().is_empty() || image.len() > 256 {
            return Err(IpcError::from(TalosError::InvalidId {
                what: "image",
                reason: "must be 1..=256 chars".to_owned(),
            }));
        }
        config.image = image.to_owned();
    }
    if let Some(width) = params.get("width").and_then(|value| value.as_i64()) {
        config.width = width;
    }
    if let Some(height) = params.get("height").and_then(|value| value.as_i64()) {
        config.height = height;
    }
    if let Some(vnc) = params.get("vncExpose").and_then(|value| value.as_bool()) {
        config.vnc_expose = vnc;
    }
    config.validate().map_err(IpcError::from)?;
    Ok(config)
}

fn task_id_of(params: &serde_json::Value) -> Result<String, IpcError> {
    let task_id = params
        .get("taskId")
        .and_then(|value| value.as_str())
        .unwrap_or("task-computer-1");
    if task_id.trim().is_empty() || task_id.len() > 128 {
        return Err(IpcError::from(TalosError::InvalidId {
            what: "task_id",
            reason: "must be 1..=128 chars".to_owned(),
        }));
    }
    Ok(task_id.to_owned())
}

/// `computer_vm_launch` — spawn (or reuse) the task container.
#[tauri::command]
pub async fn computer_vm_launch(
    state: tauri::State<'_, AppState>,
    params: serde_json::Value,
) -> Result<VmSessionView, IpcError> {
    let task_id = task_id_of(&params)?;
    let config = config_from_params(&params)?;
    manager()
        .ensure_running(&task_id, &state.tool_root, &config)
        .await
        .map_err(IpcError::from)
}

/// `computer_vm_status` — current session view (never spins anything up).
#[tauri::command]
pub async fn computer_vm_status() -> Result<VmSessionView, IpcError> {
    Ok(manager().status().await)
}

/// `computer_vm_destroy` — force-remove the container (idempotent).
#[tauri::command]
pub async fn computer_vm_destroy() -> Result<bool, IpcError> {
    manager().destroy().await.map_err(IpcError::from)
}

/// `computer_vm_frame` — current guest frame for the VNC viewer.
#[tauri::command]
pub async fn computer_vm_frame(with_png: Option<bool>) -> Result<VmFrame, IpcError> {
    manager()
        .frame(with_png.unwrap_or(true))
        .await
        .map_err(IpcError::from)
}

/// `computer_vm_pause` — freeze all guest processes (emergency-stop path).
#[tauri::command]
pub async fn computer_vm_pause() -> Result<bool, IpcError> {
    manager().pause_all().await.map_err(IpcError::from)
}

/// `computer_vm_resume` — unfreeze guest processes (explicit reset path).
#[tauri::command]
pub async fn computer_vm_resume() -> Result<bool, IpcError> {
    manager().resume_all().await.map_err(IpcError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vm_launch_params_validate() {
        assert!(config_from_params(&serde_json::json!({})).is_ok());
        assert!(config_from_params(&serde_json::json!({ "vncExpose": true })).is_ok());
        assert!(config_from_params(&serde_json::json!({ "image": "" })).is_err());
        assert!(config_from_params(&serde_json::json!({ "width": 100 })).is_err());
        assert!(task_id_of(&serde_json::json!({})).is_ok());
        assert!(task_id_of(&serde_json::json!({ "taskId": "" })).is_err());
    }
}
