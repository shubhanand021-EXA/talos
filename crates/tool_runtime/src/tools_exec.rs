//! `process.exec`: allowlisted subprocess execution inside the sandbox root.
//!
//! Safety: exact-name allowlist, denylist for destructive/network-pipe
//! patterns, no shell (argv-exact spawn), minimal environment (no user
//! secrets), per-workspace cwd, timeout (default 60s, max 300s), truncated
//! output capture (100KB per stream) with content hashes.
//! Dry-run always succeeds (planning); live execution requires
//! `exec_enabled` plus an allowlist hit, otherwise a structured `Disabled`
//! outcome — never a silent pass.

use sha2::{Digest, Sha256};
use talos_common::{Result, TalosError};

use crate::adapters::{check_command_allowed, SandboxConfig};

fn err(msg: &str) -> TalosError {
    TalosError::Storage(msg.to_owned())
}

fn hash_text(text: &str) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(text.as_bytes())))
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
///
/// Environment is minimal: the parent environment is cleared and only a safe
/// `PATH` (plus `SystemRoot`/`LANG` where present) is restored, so user
/// secrets never leak into the child.
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
    check_command_allowed(command, args)?;
    if !config.exec_allowlist.iter().any(|name| name == command) {
        return Err(err(&format!(
            "process.exec: '{command}' is not allowlisted"
        )));
    }
    if command.contains('/') || command.contains('\\') {
        return Err(err("process.exec: path separators are rejected"));
    }
    let timeout = std::time::Duration::from_millis(timeout_ms.clamp(100, 300_000));
    let started = std::time::Instant::now();
    let mut child = tokio::process::Command::new(command);
    child.args(args);
    child.current_dir(&config.root);
    child.stdout(std::process::Stdio::piped());
    child.stderr(std::process::Stdio::piped());
    child.stdin(std::process::Stdio::null());
    child.env_clear();
    if let Some(path) = std::env::var_os("PATH") {
        child.env("PATH", path);
    }
    #[cfg(windows)]
    if let Some(root) = std::env::var_os("SystemRoot") {
        child.env("SystemRoot", root);
    }
    if let Some(lang) = std::env::var_os("LANG") {
        child.env("LANG", lang);
    }
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
    let cap = config.max_output_bytes.max(1);
    let truncate = |bytes: Vec<u8>| -> (String, bool) {
        let truncated = bytes.len() > cap;
        let capped = if truncated {
            bytes[..cap].to_vec()
        } else {
            bytes
        };
        let mut text = String::from_utf8_lossy(&capped).into_owned();
        if truncated {
            text.push_str("\n…[truncated]");
        }
        (text, truncated)
    };
    let (stdout, out_truncated) = truncate(output.stdout);
    let (stderr, err_truncated) = truncate(output.stderr);
    Ok(serde_json::json!({
        "command": command,
        "exitCode": status.code(),
        "stdout": stdout,
        "stderr": stderr,
        "stdoutHash": hash_text(&stdout),
        "stderrHash": hash_text(&stderr),
        "truncated": out_truncated || err_truncated,
        "timedOut": false,
        "durationMs": started.elapsed().as_millis() as u64,
        "workspacePath": config.root.to_string_lossy(),
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

    #[tokio::test]
    async fn denylist_rejects_even_when_allowlisted() {
        let config = SandboxConfig {
            exec_enabled: true,
            exec_allowlist: vec!["sudo".to_owned(), "curl".to_owned()],
            ..SandboxConfig::default()
        };
        assert!(exec_live(&config, "sudo", &[], 1_000).await.is_err());
        assert!(exec_live(
            &config,
            "curl",
            &[
                "https://example.com/x".to_owned(),
                "|".to_owned(),
                "bash".to_owned()
            ],
            1_000,
        )
        .await
        .is_err());
    }

    #[tokio::test]
    async fn allowlist_still_enforced() {
        let config = SandboxConfig {
            exec_enabled: true,
            exec_allowlist: vec!["echo".to_owned()],
            ..SandboxConfig::default()
        };
        assert!(exec_live(&config, "not-allowlisted", &[], 1_000)
            .await
            .unwrap_err()
            .to_string()
            .contains("allowlisted"));
    }
}
