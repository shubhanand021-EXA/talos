//! `process.exec`: disabled-by-default subprocess execution.
//!
//! Safety: exact-name allowlist, timeout, truncated output capture, no shell.
//! Dry-run always succeeds (planning); live execution requires
//! `exec_enabled` plus an allowlist hit, otherwise a structured `Disabled`
//! outcome — never a silent pass.

use talos_common::{Result, TalosError};

use crate::adapters::SandboxConfig;

fn err(msg: &str) -> TalosError {
    TalosError::Storage(msg.to_owned())
}

/// Planned exec effects (dry-run: never spawns).
#[must_use]
pub fn exec_plan(command: &str, args: &[String], timeout_ms: u64) -> serde_json::Value {
    serde_json::json!({
        "command": command,
        "args": args,
        "timeoutMs": timeout_ms,
        "planned": true,
    })
}

/// Execute an allowlisted command with timeout and truncated capture.
pub async fn exec_live(
    config: &SandboxConfig,
    command: &str,
    args: &[String],
    timeout_ms: u64,
) -> Result<serde_json::Value> {
    if !config.exec_enabled {
        return Err(err(
            "process.exec is disabled by default; enable explicitly with an allowlist",
        ));
    }
    if !config.exec_allowlist.iter().any(|name| name == command) {
        return Err(err(&format!(
            "process.exec: '{command}' is not allowlisted"
        )));
    }
    if command.contains('/') || command.contains('\\') {
        return Err(err("process.exec: path separators are rejected"));
    }
    let timeout = std::time::Duration::from_millis(timeout_ms.max(100));
    let mut child = tokio::process::Command::new(command);
    child.args(args);
    child.current_dir(&config.root);
    child.stdout(std::process::Stdio::piped());
    child.stderr(std::process::Stdio::piped());
    child.stdin(std::process::Stdio::null());
    // No shell: argv-exact spawn only.
    let mut spawned = child
        .spawn()
        .map_err(|e| err(&format!("process.exec spawn: {e}")))?;
    let status = tokio::time::timeout(timeout, spawned.wait())
        .await
        .map_err(|_| err("process.exec: timed out"))?
        .map_err(|e| err(&format!("process.exec wait: {e}")))?;
    let output = spawned
        .wait_with_output()
        .await
        .map_err(|e| err(&format!("process.exec capture: {e}")))?;
    let truncate = |bytes: Vec<u8>| -> String {
        let capped = if bytes.len() > 32_768 {
            bytes[..32_768].to_vec()
        } else {
            bytes
        };
        String::from_utf8_lossy(&capped).into_owned()
    };
    Ok(serde_json::json!({
        "command": command,
        "exitCode": status.code(),
        "stdout": truncate(output.stdout),
        "stderr": truncate(output.stderr),
        "timedOut": false,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn disabled_by_default() {
        let config = SandboxConfig::default();
        assert!(!config.exec_enabled);
        let outcome = exec_live(&config, "echo", &[], 1_000).await;
        assert!(outcome.is_err());
        assert!(outcome.unwrap_err().to_string().contains("disabled"));
    }

    #[test]
    fn dry_run_plans() {
        let planned = exec_plan("echo", &["hi".to_owned()], 1_000);
        assert_eq!(planned["planned"], true);
    }
}
