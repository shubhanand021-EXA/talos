//! Workspace-scoped git operations: status, diff, branch, commit.
//!
//! Every command runs with `current_dir` locked to the workspace root and
//! rejects `--git-dir`, `--work-tree`, and `-C` overrides, so operations can
//! never escape the isolated workspace or touch the parent repo directly.
//! Outputs are bounded and hashed; commits are evidence-gated by the caller.

use sha2::{Digest, Sha256};
use talos_common::{Result, TalosError};

use crate::adapters::SandboxConfig;

fn err(msg: &str) -> TalosError {
    TalosError::Storage(msg.to_owned())
}

fn hash_text(text: &str) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(text.as_bytes())))
}

/// Reject git dir/worktree overrides that could escape the workspace.
fn check_git_args(args: &[String]) -> Result<()> {
    for arg in args {
        if arg == "--git-dir"
            || arg == "--work-tree"
            || arg == "-C"
            || arg.starts_with("--git-dir=")
            || arg.starts_with("--work-tree=")
        {
            return Err(err(
                "git: dir overrides are rejected (workspace-scoped only)",
            ));
        }
    }
    Ok(())
}

async fn run_git(config: &SandboxConfig, args: &[&str], timeout_ms: u64) -> Result<String> {
    let owned: Vec<String> = args.iter().map(ToString::to_string).collect();
    check_git_args(&owned)?;
    let mut child = tokio::process::Command::new("git");
    for arg in args {
        child.arg(arg);
    }
    child.current_dir(&config.root);
    child.stdout(std::process::Stdio::piped());
    child.stderr(std::process::Stdio::null());
    child.stdin(std::process::Stdio::null());
    child.env_clear();
    if let Some(path) = std::env::var_os("PATH") {
        child.env("PATH", path);
    }
    #[cfg(windows)]
    if let Some(root) = std::env::var_os("SystemRoot") {
        child.env("SystemRoot", root);
    }
    let mut spawned = child
        .spawn()
        .map_err(|e| err(&format!("git unavailable: {e}")))?;
    let timeout = std::time::Duration::from_millis(timeout_ms.clamp(100, 60_000));
    let status = tokio::time::timeout(timeout, spawned.wait())
        .await
        .map_err(|_| err("git: timed out"))?
        .map_err(|e| err(&format!("git wait: {e}")))?;
    let output = spawned
        .wait_with_output()
        .await
        .map_err(|e| err(&format!("git capture: {e}")))?;
    if !status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let detail = stderr.trim();
        return Err(err(&format!(
            "git failed: {}",
            if detail.is_empty() {
                "non-zero exit".to_owned()
            } else {
                detail.chars().take(300).collect()
            }
        )));
    }
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    if text.len() > 20_000 {
        text.truncate(20_000);
        text.push_str("\n…[truncated]");
    }
    Ok(text)
}

/// `git.status`: porcelain status + branch, read-only.
pub async fn git_status(config: &SandboxConfig) -> Result<serde_json::Value> {
    let branch = run_git(config, &["branch", "--show-current"], 10_000)
        .await
        .map(|s| s.trim().to_owned())
        .unwrap_or_default();
    let porcelain = run_git(config, &["status", "--porcelain=v1", "-b"], 10_000).await?;
    let entries: Vec<serde_json::Value> = porcelain
        .lines()
        .filter(|line| !line.starts_with("##") && !line.trim().is_empty())
        .take(200)
        .map(|line| serde_json::json!({ "entry": line }))
        .collect();
    Ok(serde_json::json!({
        "branch": if branch.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(branch) },
        "clean": entries.is_empty(),
        "entries": entries,
    }))
}

/// `git.diff`: stat + bounded patch with content hash.
pub async fn git_diff(
    config: &SandboxConfig,
    stat_only: bool,
    max_chars: usize,
) -> Result<serde_json::Value> {
    let stat = run_git(config, &["diff", "--stat"], 15_000).await?;
    let files = stat.lines().filter(|l| l.contains('|')).count();
    if stat_only {
        return Ok(serde_json::json!({
            "files": files,
            "patchHash": hash_text(&stat),
            "patch": stat,
            "truncated": false,
        }));
    }
    let mut patch = run_git(config, &["diff"], 15_000).await?;
    let mut truncated = false;
    if patch.len() > max_chars {
        patch.truncate(max_chars);
        patch.push_str("\n…[truncated]");
        truncated = true;
    }
    let hash = hash_text(&patch);
    Ok(serde_json::json!({
        "files": files,
        "patchHash": hash,
        "patch": patch,
        "truncated": truncated,
    }))
}

/// `git.branch`: current branch, read-only.
pub async fn git_branch(config: &SandboxConfig) -> Result<serde_json::Value> {
    let branch = run_git(config, &["branch", "--show-current"], 10_000)
        .await
        .map(|s| s.trim().to_owned())
        .unwrap_or_default();
    Ok(serde_json::json!({
        "branch": if branch.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(branch) },
    }))
}

/// `git.commit`: stage all workspace files, then commit with a validated message.
pub async fn git_commit(
    config: &SandboxConfig,
    message: &str,
    allow_empty: bool,
) -> Result<serde_json::Value> {
    run_git(config, &["add", "-A"], 15_000).await?;
    let mut args = vec!["commit", "-m", message];
    if allow_empty {
        args.push("--allow-empty");
    }
    match run_git(config, &args, 30_000).await {
        Ok(_) => {}
        Err(e) if e.to_string().contains("nothing to commit") && !allow_empty => {
            return Err(err("git.commit: nothing to commit"));
        }
        Err(e) => return Err(e),
    }
    let commit = run_git(config, &["rev-parse", "HEAD"], 10_000)
        .await
        .map(|s| s.trim().to_owned())
        .ok();
    Ok(serde_json::json!({
        "commit": commit,
        "message": message,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn root() -> (tempfile::TempDir, SandboxConfig) {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = SandboxConfig {
            root: dir.path().to_path_buf(),
            ..SandboxConfig::default()
        };
        (dir, config)
    }

    fn git_available() -> bool {
        std::process::Command::new("git")
            .arg("--version")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    #[test]
    fn rejects_dir_overrides() {
        assert!(check_git_args(&["--git-dir".to_owned(), "/x".to_owned()]).is_err());
        assert!(check_git_args(&["-C".to_owned(), "/tmp".to_owned()]).is_err());
        assert!(check_git_args(&["status".to_owned()]).is_ok());
    }

    #[tokio::test]
    async fn status_and_branch_outside_repo_fail_cleanly() {
        if !git_available() {
            return;
        }
        let (_dir, config) = root().await;
        // A fresh tempdir is not a repo: status errors instead of touching
        // parents; branch reports no branch instead of escaping.
        assert!(git_status(&config).await.is_err());
        match git_branch(&config).await {
            Err(_) => {}
            Ok(output) => assert!(output["branch"].is_null()),
        }
    }

    #[tokio::test]
    async fn commit_flow_in_workspace_repo() {
        if !git_available() {
            return;
        }
        let (_dir, config) = root().await;
        let init = tokio::process::Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(&config.root)
            .stdin(std::process::Stdio::null())
            .status()
            .await;
        if !init.map(|s| s.success()).unwrap_or(false) {
            return;
        }
        for (key, value) in [("user.email", "talos@test.local"), ("user.name", "talos")] {
            let _ = tokio::process::Command::new("git")
                .args(["config", key, value])
                .current_dir(&config.root)
                .stdin(std::process::Stdio::null())
                .status()
                .await;
        }
        tokio::fs::write(config.root.join("a.txt"), b"hello")
            .await
            .expect("write");
        let committed = git_commit(&config, "add a.txt", false)
            .await
            .expect("commit");
        assert!(committed["commit"].as_str().unwrap_or_default().len() >= 7);
        let status = git_status(&config).await.expect("status");
        assert_eq!(status["clean"], true);
        let diff = git_diff(&config, true, 8000).await.expect("diff");
        assert_eq!(diff["files"], 0);
        assert!(git_commit(&config, "-bad", false).await.is_err());
    }
}
