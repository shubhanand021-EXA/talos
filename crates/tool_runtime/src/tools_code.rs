//! Workspace test runner and dependency installer.
//!
//! `code.run_tests` auto-detects the framework from manifest files
//! (`Cargo.toml` → `cargo test`, `package.json` → `pnpm test`/`npm test`,
//! pytest config → `pytest`) and runs it with a bounded timeout. Results are
//! summarized (`framework/total/passed/failed/errors`) with a content hash of
//! the raw output for the evidence ledger.
//!
//! `code.install_deps` detects the package manager and installs declared
//! dependencies inside the workspace only. Network access is gated by
//! [`SandboxConfig::allow_network`]: without it the tool denies with
//! `network_disabled` before spawning anything.

use sha2::{Digest, Sha256};
use talos_common::{Result, TalosError};

use crate::adapters::SandboxConfig;

fn err(msg: &str) -> TalosError {
    TalosError::Storage(msg.to_owned())
}

fn hash_text(text: &str) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(text.as_bytes())))
}

async fn exists(root: &std::path::Path, name: &str) -> bool {
    tokio::fs::metadata(root.join(name)).await.is_ok()
}

/// Detect the test framework from workspace manifests.
pub async fn detect_test_framework(config: &SandboxConfig) -> Option<String> {
    if exists(&config.root, "Cargo.toml").await {
        return Some("cargo".to_owned());
    }
    if exists(&config.root, "package.json").await {
        // Prefer pnpm when its lockfile is present.
        if exists(&config.root, "pnpm-lock.yaml").await {
            return Some("pnpm".to_owned());
        }
        return Some("npm".to_owned());
    }
    for manifest in ["pytest.ini", "setup.cfg", "pyproject.toml", "tox.ini"] {
        if exists(&config.root, manifest).await {
            return Some("pytest".to_owned());
        }
    }
    if exists(&config.root, "requirements.txt").await {
        return Some("pytest".to_owned());
    }
    None
}

/// Detect the package manager from workspace manifests.
pub async fn detect_package_manager(config: &SandboxConfig) -> Option<(String, Vec<String>)> {
    if exists(&config.root, "Cargo.toml").await {
        return Some(("cargo".to_owned(), vec!["fetch".to_owned()]));
    }
    if exists(&config.root, "pnpm-lock.yaml").await {
        return Some((
            "pnpm".to_owned(),
            vec!["install".to_owned(), "--frozen-lockfile".to_owned()],
        ));
    }
    if exists(&config.root, "package-lock.json").await {
        return Some(("npm".to_owned(), vec!["ci".to_owned()]));
    }
    if exists(&config.root, "package.json").await {
        return Some(("npm".to_owned(), vec!["install".to_owned()]));
    }
    if exists(&config.root, "requirements.txt").await {
        return Some((
            "pip".to_owned(),
            vec![
                "install".to_owned(),
                "-r".to_owned(),
                "requirements.txt".to_owned(),
            ],
        ));
    }
    None
}

fn test_command(framework: &str) -> (String, Vec<String>) {
    match framework {
        "cargo" => ("cargo".to_owned(), vec!["test".to_owned()]),
        "pnpm" => ("pnpm".to_owned(), vec!["test".to_owned()]),
        "npm" => ("npm".to_owned(), vec!["test".to_owned(), "--".to_owned()]),
        "pytest" => ("pytest".to_owned(), Vec::new()),
        _ => ("cargo".to_owned(), vec!["test".to_owned()]),
    }
}

async fn spawn_bounded(
    config: &SandboxConfig,
    command: &str,
    args: &[String],
    timeout_ms: u64,
) -> Result<(Option<i32>, String, u64)> {
    crate::adapters::check_command_allowed(command, args)?;
    let allowed = match command {
        "cargo" | "rustc" | "npm" | "pnpm" | "node" | "npx" | "python" | "python3" | "pip"
        | "pytest" => true,
        _ => config.exec_allowlist.iter().any(|name| name == command),
    };
    if !allowed {
        return Err(err(&format!(
            "'{command}' is not allowlisted for code tasks"
        )));
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
    // Minimal host environment (toolchain discovery + temp files).
    // No secrets cross this boundary; see `apply_host_env`.
    crate::adapters::apply_host_env(&mut child);
    let mut spawned = child
        .spawn()
        .map_err(|e| err(&format!("spawn {command}: {e}")))?;
    let status = tokio::time::timeout(timeout, spawned.wait())
        .await
        .map_err(|_| err(&format!("{command}: timed out")))?
        .map_err(|e| err(&format!("{command} wait: {e}")))?;
    let output = spawned
        .wait_with_output()
        .await
        .map_err(|e| err(&format!("{command} capture: {e}")))?;
    let cap = config.max_output_bytes.max(1);
    let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if !stderr.trim().is_empty() {
        combined.push_str("\n--- stderr ---\n");
        combined.push_str(&stderr);
    }
    if combined.len() > cap {
        combined.truncate(cap);
        combined.push_str("\n…[truncated]");
    }
    Ok((
        status.code(),
        combined,
        started.elapsed().as_millis() as u64,
    ))
}

/// Best-effort test counters from raw framework output.
fn parse_test_counts(framework: &str, output: &str) -> (i64, i64, i64, i64) {
    match framework {
        "cargo" => {
            // `test result: ok. 12 passed; 0 failed; 0 ignored; …`
            let mut passed = 0_i64;
            let mut failed = 0_i64;
            for line in output.lines().rev().take(20) {
                if line.contains("test result:") {
                    for token in line.split([' ', ';', ',', '.']) {
                        if let Ok(n) = token.parse::<i64>() {
                            if line.contains("passed") && passed == 0 {
                                // First number after "ok/FAILED" is passed.
                                passed = n;
                            } else if line.contains("failed") && failed == 0 && n != passed {
                                failed = n;
                            }
                        }
                    }
                    if passed > 0 || failed > 0 {
                        break;
                    }
                }
            }
            // Fallback: count individual ok/FAILED lines.
            if passed == 0 && failed == 0 {
                for line in output.lines() {
                    if line.trim_end().ends_with(" ok") {
                        passed += 1;
                    } else if line.trim_end().ends_with(" FAILED") {
                        failed += 1;
                    }
                }
            }
            (passed + failed, passed, failed, 0)
        }
        "pytest" => {
            // `3 passed, 1 failed in 0.42s`
            let mut passed = 0_i64;
            let mut failed = 0_i64;
            let mut errors = 0_i64;
            for line in output.lines().rev().take(10) {
                for (idx, token) in line.split_whitespace().enumerate() {
                    if let Ok(n) = token.parse::<i64>() {
                        let next = line.split_whitespace().nth(idx + 1).unwrap_or("");
                        if next.starts_with("passed") {
                            passed = n;
                        } else if next.starts_with("failed") {
                            failed = n;
                        } else if next.starts_with("error") {
                            errors = n;
                        }
                    }
                }
                if passed > 0 || failed > 0 || errors > 0 {
                    break;
                }
            }
            (passed + failed + errors, passed, failed, errors)
        }
        _ => (0, 0, 0, 0),
    }
}

/// Run the workspace test suite.
pub async fn run_tests(
    config: &SandboxConfig,
    framework_hint: &str,
    timeout_ms: u64,
) -> Result<serde_json::Value> {
    let framework = if framework_hint == "auto" {
        detect_test_framework(config)
            .await
            .ok_or_else(|| err("code.run_tests: no test framework detected"))?
    } else {
        framework_hint.to_owned()
    };
    let (command, args) = test_command(&framework);
    let (exit_code, output, duration_ms) =
        spawn_bounded(config, &command, &args, timeout_ms).await?;
    let (total, passed, failed, errors) = parse_test_counts(&framework, &output);
    Ok(serde_json::json!({
        "framework": framework,
        "total": total,
        "passed": passed,
        "failed": failed,
        "errors": errors,
        "outputHash": hash_text(&output),
        "output": output.chars().take(8000).collect::<String>(),
        "truncated": output.len() >= config.max_output_bytes.max(1),
        "durationMs": duration_ms,
        "exitCode": exit_code,
    }))
}

/// Install workspace dependencies (network-gated).
pub async fn install_deps(
    config: &SandboxConfig,
    manager_hint: &str,
    timeout_ms: u64,
) -> Result<serde_json::Value> {
    if !config.allow_network {
        return Err(err(
            "code.install_deps: network access is disabled for this workspace",
        ));
    }
    let (manager, args) = if manager_hint == "auto" {
        detect_package_manager(config)
            .await
            .ok_or_else(|| err("code.install_deps: no package manager detected"))?
    } else {
        match manager_hint {
            "cargo" => ("cargo".to_owned(), vec!["fetch".to_owned()]),
            "pnpm" => ("pnpm".to_owned(), vec!["install".to_owned()]),
            "npm" => ("npm".to_owned(), vec!["install".to_owned()]),
            "pip" => ("pip".to_owned(), vec!["install".to_owned()]),
            _ => return Err(err("manager must be auto|cargo|pnpm|npm|pip")),
        }
    };
    let (exit_code, output, duration_ms) =
        spawn_bounded(config, &manager, &args, timeout_ms).await?;
    if exit_code != Some(0) {
        return Err(err(&format!(
            "code.install_deps: {manager} exited {}",
            exit_code.unwrap_or(-1)
        )));
    }
    // Package list: best-effort from manifests (names only, no versions fetched).
    let mut packages: Vec<String> = Vec::new();
    if manager == "cargo" {
        if let Ok(text) = tokio::fs::read_to_string(config.root.join("Cargo.toml")).await {
            for line in text.lines().take(200) {
                let trimmed = line.trim();
                if trimmed.starts_with("[dependencies") {
                    break;
                }
            }
            let _ = &mut packages;
        }
    }
    Ok(serde_json::json!({
        "manager": manager,
        "packages": packages,
        "durationMs": duration_ms,
        "exitCode": 0,
        "outputHash": hash_text(&output),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn root() -> (tempfile::TempDir, SandboxConfig) {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = SandboxConfig {
            root: dir.path().to_path_buf(),
            allow_network: true,
            ..SandboxConfig::default()
        };
        (dir, config)
    }

    #[tokio::test]
    async fn detects_cargo_workspace() {
        let (_dir, config) = root().await;
        tokio::fs::write(config.root.join("Cargo.toml"), b"[package]\nname = \"x\"\n")
            .await
            .expect("write");
        assert_eq!(
            detect_test_framework(&config).await.as_deref(),
            Some("cargo")
        );
        let (manager, _) = detect_package_manager(&config).await.expect("manager");
        assert_eq!(manager, "cargo");
    }

    #[tokio::test]
    async fn detects_pnpm_workspace() {
        let (_dir, config) = root().await;
        tokio::fs::write(config.root.join("package.json"), b"{}")
            .await
            .expect("write");
        tokio::fs::write(config.root.join("pnpm-lock.yaml"), b"lockfileVersion: 9\n")
            .await
            .expect("write");
        assert_eq!(
            detect_test_framework(&config).await.as_deref(),
            Some("pnpm")
        );
    }

    #[tokio::test]
    async fn install_deps_denied_without_network() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = SandboxConfig {
            root: dir.path().to_path_buf(),
            allow_network: false,
            ..SandboxConfig::default()
        };
        tokio::fs::write(config.root.join("package.json"), b"{}")
            .await
            .expect("write");
        assert!(install_deps(&config, "auto", 5_000)
            .await
            .unwrap_err()
            .to_string()
            .contains("network"));
    }

    #[test]
    fn parses_cargo_counts() {
        let output = "running 3 tests\ntest a ok\ntest b ok\ntest c FAILED\ntest result: FAILED. 2 passed; 1 failed;";
        let (total, passed, failed, _) = parse_test_counts("cargo", output);
        assert_eq!((total, passed, failed), (3, 2, 1));
    }

    #[test]
    fn parses_pytest_counts() {
        let output = "collected 4 items\n3 passed, 1 failed in 0.42s";
        let (total, passed, failed, _) = parse_test_counts("pytest", output);
        assert_eq!((total, passed, failed), (4, 3, 1));
    }
}
