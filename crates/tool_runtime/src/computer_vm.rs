//! Ephemeral-VM execution glue for live computer tools.
//!
//! When [`SandboxConfig::computer_backend`] is `EphemeralVm`, live actions
//! route here instead of the host OS or the mock: the [`VmManager`] spawns
//! (or reuses) a per-task Linux desktop container, translates the resolved
//! [`DriverAction`] to `xdotool`, and returns receipt-shaped output with
//! frame hashes. Selectors are rejected in the VM (no accessibility tree
//! inside the guest — capture the screen and click coordinates instead).

use computer_use::ComputerBackend;

use crate::adapters::SandboxConfig;
use crate::model::{ToolErrorDetail, ToolStatus};

fn err(code: &str, message: String) -> ToolErrorDetail {
    ToolErrorDetail::new(code, message)
}

/// Map a VM failure message to a stable receipt code.
///
/// The manager already prefixes machine-readable codes (`vm_*`,
/// `export_*`); anything else is an IO failure.
fn vm_code(message: &str) -> (String, String) {
    // TalosError::Storage displays as `storage error: {code}: {detail}`;
    // strip the envelope prefix before reading the machine-readable code.
    let stripped = message.strip_prefix("storage error: ").unwrap_or(message);
    let head = stripped.split(':').next().unwrap_or("").trim();
    if head.starts_with("vm_") || head.starts_with("export_") {
        (head.to_owned(), message.to_owned())
    } else {
        ("io_error".to_owned(), message.to_owned())
    }
}

/// Resolve the VM manager: per-config override (tests) or the
/// process-global production manager.
fn manager_for(config: &SandboxConfig) -> std::sync::Arc<computer_use::vm::VmManager> {
    config
        .computer_vm
        .clone()
        .unwrap_or_else(computer_use::vm::VmManager::global)
}

/// Ensure the task's VM session (spawn on demand, reuse when warm).
pub async fn ensure_session(
    config: &SandboxConfig,
    task_id: &str,
) -> Result<computer_use::vm::VmSessionView, String> {
    if config.computer_backend != ComputerBackend::EphemeralVm {
        return Err("vm backend not selected".to_owned());
    }
    let envelope = config.computer.config();
    let vm_config = computer_use::vm::VmConfig {
        width: envelope.viewport_width.clamp(640, 3840),
        height: envelope.viewport_height.clamp(480, 2160),
        ..computer_use::vm::VmConfig::default()
    };
    manager_for(config)
        .ensure_running(task_id, &config.root, &vm_config)
        .await
        .map_err(|error| error.to_string())
}

/// Execute one live computer tool inside the ephemeral VM.
pub async fn execute_tool(
    manifest_id: &str,
    task_id: &str,
    params: &serde_json::Value,
    config: &SandboxConfig,
    warnings: Vec<String>,
) -> (
    ToolStatus,
    serde_json::Value,
    Option<ToolErrorDetail>,
    Option<crate::adapters::RemoteExecution>,
) {
    let failed = |code: &str, message: String| {
        (
            ToolStatus::Failed,
            serde_json::json!({}),
            Some(err(code, format!("{manifest_id}: {message}"))),
            None,
        )
    };
    // The VM has no accessibility tree: selector-based targeting cannot
    // resolve inside the guest. Fail fast with guidance instead of
    // fabricating a resolution report.
    if params
        .get("selector")
        .and_then(|value| value.as_str())
        .is_some()
    {
        return failed(
            "invalid_params",
            "selectors are unsupported in the ephemeral VM (no guest accessibility tree); capture the screen and use coordinates".to_owned(),
        );
    }
    let action = match computer_use::real::plan_action(manifest_id, params) {
        Ok((action, _)) => action,
        Err(error) => return failed("invalid_params", error.to_string()),
    };
    if let Err(error) = ensure_session(config, task_id).await {
        let (code, message) = vm_code(&error);
        return failed(code.as_str(), message);
    }
    let manager = manager_for(config);
    // Observe (1): frame before acting, so click-like verification is honest.
    let before = match manager.frame(false).await {
        Ok(frame) => frame,
        Err(error) => {
            let (code, message) = vm_code(&error.to_string());
            return failed(code.as_str(), message);
        }
    };
    if let Err(error) = manager.act(&action).await {
        let (code, message) = vm_code(&error.to_string());
        return failed(code.as_str(), message);
    }
    // Observe (2): fresh frame with PNG for the receipt thumbnail.
    let after = match manager.frame(true).await {
        Ok(frame) => frame,
        Err(error) => {
            let (code, message) = vm_code(&error.to_string());
            return failed(code.as_str(), message);
        }
    };
    manager.touch().await;
    let _ = config
        .computer
        .note_session_action(task_id)
        .await
        .map_err(|violation| violation.message.clone());
    let click_like = matches!(
        action,
        computer_use::driver::DriverAction::Click { .. }
            | computer_use::driver::DriverAction::DoubleClick { .. }
            | computer_use::driver::DriverAction::Drag { .. }
    );
    let verification_failed = click_like && before.hash == after.hash;
    let mut output = serde_json::json!({
        "simulated": false,
        "backend": "ephemeral-vm",
        "screenHash": after.hash,
        "beforeHash": before.hash,
        "afterHash": after.hash,
        "verified": !verification_failed,
        "verificationFailed": verification_failed,
        "focusLost": false,
        "boundingBox": null,
        "confidence": null,
        "viewport": { "width": after.width, "height": after.height },
    });
    if let Some(thumbnail) = thumbnail_for(&after).await {
        output["thumbnail"] = serde_json::Value::String(thumbnail);
    }
    if !warnings.is_empty() {
        output["warnings"] = serde_json::Value::Array(
            warnings
                .into_iter()
                .map(serde_json::Value::String)
                .collect(),
        );
    }
    (ToolStatus::Success, output, None, None)
}

/// Read-only screen capture inside the ephemeral VM.
pub async fn capture_tool(
    task_id: &str,
    config: &SandboxConfig,
) -> (
    ToolStatus,
    serde_json::Value,
    Option<ToolErrorDetail>,
    Option<crate::adapters::RemoteExecution>,
) {
    let failed = |code: &str, message: String| {
        (
            ToolStatus::Failed,
            serde_json::json!({}),
            Some(err(code, format!("computer.capture_screen: {message}"))),
            None,
        )
    };
    if let Err(error) = ensure_session(config, task_id).await {
        let (code, message) = vm_code(&error);
        return failed(code.as_str(), message);
    }
    let manager = manager_for(config);
    let frame = match manager.frame(true).await {
        Ok(frame) => frame,
        Err(error) => {
            let (code, message) = vm_code(&error.to_string());
            return failed(code.as_str(), message);
        }
    };
    manager.touch().await;
    let _ = config
        .computer
        .note_session_action(task_id)
        .await
        .map_err(|violation| violation.message.clone());
    let mut output = serde_json::json!({
        "simulated": false,
        "backend": "ephemeral-vm",
        "viewport": { "width": frame.width, "height": frame.height },
        "screenHash": frame.hash,
    });
    if let Some(thumbnail) = frame.png_base64 {
        output["thumbnail"] =
            serde_json::Value::String(format!("data:image/png;base64,{thumbnail}"));
    }
    (ToolStatus::Success, output, None, None)
}

/// Read-only element lookup inside the ephemeral VM.
///
/// The guest exposes no accessibility tree, so selectors cannot resolve:
/// the receipt carries a fresh frame plus an honest miss. Agents locate
/// targets by capturing the screen and clicking coordinates.
pub async fn locate_tool(
    task_id: &str,
    selector: &str,
    config: &SandboxConfig,
) -> (
    ToolStatus,
    serde_json::Value,
    Option<ToolErrorDetail>,
    Option<crate::adapters::RemoteExecution>,
) {
    let (status, mut output, error, remote) = capture_tool(task_id, config).await;
    if status != ToolStatus::Success {
        return (status, output, error, remote);
    }
    if let Some(object) = output.as_object_mut() {
        object.insert(
            "selector".to_owned(),
            serde_json::Value::String(selector.to_owned()),
        );
        object.insert(
            "resolved".to_owned(),
            serde_json::json!({
                "selector": selector,
                "found": false,
                "visible": false,
                "interactable": false,
                "confidence": 0.0,
                "note": "no accessibility tree in the ephemeral VM; capture the screen and use coordinates",
            }),
        );
        object.insert("boundingBox".to_owned(), serde_json::Value::Null);
    }
    (ToolStatus::Success, output, None, None)
}

/// Copy a verified artifact out of the guest export dir into the task root.
pub async fn export_tool(
    task_id: &str,
    path: &str,
    max_bytes: usize,
    config: &SandboxConfig,
) -> (
    ToolStatus,
    serde_json::Value,
    Option<ToolErrorDetail>,
    Option<crate::adapters::RemoteExecution>,
) {
    let failed = |code: &str, message: String| {
        (
            ToolStatus::Failed,
            serde_json::json!({}),
            Some(err(code, format!("computer.export_file: {message}"))),
            None,
        )
    };
    if config.computer_backend == ComputerBackend::NativeOs {
        return (
            ToolStatus::Denied,
            serde_json::json!({}),
            Some(err(
                "native_os_deprecated",
                "native OS control is deprecated; use the ephemeral VM backend".to_owned(),
            )),
            None,
        );
    }
    if config.computer_backend != ComputerBackend::EphemeralVm {
        // Mock path: deterministic record, no container.
        return (
            ToolStatus::Success,
            serde_json::json!({
                "path": path,
                "bytes": 0,
                "hash": computer_use::MockComputer::screen_hash(task_id, "export_file", path),
                "artifact": null,
                "simulated": true,
            }),
            None,
            None,
        );
    }
    if let Err(error) = ensure_session(config, task_id).await {
        let (code, message) = vm_code(&error);
        return failed(code.as_str(), message);
    }
    let file_name = std::path::Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "artifact".to_owned());
    let host_dest = config.root.join("vm-exports").join(&file_name);
    let manager = manager_for(config);
    let (bytes, hash) = match manager.export_file(path, &host_dest, &config.root).await {
        Ok(outcome) => outcome,
        Err(error) => {
            let message = error.to_string();
            let (code, detail) = vm_code(&message);
            return (
                ToolStatus::Failed,
                serde_json::json!({}),
                Some(err(code.as_str(), detail)),
                None,
            );
        }
    };
    if bytes > max_bytes as u64 {
        let _ = tokio::fs::remove_file(&host_dest).await;
        return failed(
            "export_rejected",
            format!("artifact {bytes} bytes exceeds maxBytes {max_bytes}"),
        );
    }
    manager.touch().await;
    let _ = config
        .computer
        .note_session_action(task_id)
        .await
        .map_err(|violation| violation.message.clone());
    (
        ToolStatus::Success,
        serde_json::json!({
            "path": path,
            "bytes": bytes,
            "hash": hash,
            "artifact": host_dest.to_string_lossy(),
            "simulated": false,
            "backend": "ephemeral-vm",
        }),
        None,
        None,
    )
}

/// Human-viewable thumbnail for a frame (bounded; omitted when too large).
async fn thumbnail_for(frame: &computer_use::vm::VmFrame) -> Option<String> {
    let png = frame.png_base64.as_ref()?;
    // Data-URI overhead on the 2 MiB frame cap would be ~2.7 MB per receipt:
    // thumbnails ride only small frames.
    if png.len() > 262_144 {
        return None;
    }
    Some(format!("data:image/png;base64,{png}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vm_codes_keep_their_prefix() {
        let (code, _) = vm_code("vm_not_running: gone");
        assert_eq!(code, "vm_not_running");
        let (code, _) = vm_code("export_rejected: nope");
        assert_eq!(code, "export_rejected");
        let (code, _) = vm_code("something else");
        assert_eq!(code, "io_error");
    }

    #[tokio::test]
    async fn mock_backend_short_circuits_without_containers() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = SandboxConfig {
            root: dir.path().to_path_buf(),
            ..SandboxConfig::default()
        };
        let (status, output, error, _) = export_tool("t", "a.md", 1024, &config).await;
        assert_eq!(status, ToolStatus::Success);
        assert!(error.is_none());
        assert_eq!(output["simulated"], true);
    }

    #[tokio::test]
    async fn native_backend_denies_export() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = SandboxConfig {
            root: dir.path().to_path_buf(),
            computer_backend: ComputerBackend::NativeOs,
            ..SandboxConfig::default()
        };
        let (status, _, error, _) = export_tool("t", "a.md", 1024, &config).await;
        assert_eq!(status, ToolStatus::Denied);
        assert_eq!(
            error.map(|detail| detail.code),
            Some("native_os_deprecated".to_owned())
        );
    }
}
