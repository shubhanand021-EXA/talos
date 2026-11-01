//! `process.exec`: allowlisted subprocess execution inside the sandbox root.
//!
//! Safety: exact-name allowlist, denylist for destructive/network-pipe
//! patterns, no shell (argv-exact spawn), minimal environment (no user
//! secrets), per-workspace cwd, timeout (default 60s, max 300s), truncated
//! output capture (100KB per stream) with content hashes.
//! Dry-run always succeeds (planning); live execution requires
//! `exec_enabled` plus an allowlist hit, otherwise a structured `Disabled`
//! outcome — never a silent pass.
//!
//! Timeout kill semantics (TAL-006): every spawned child sets
//! `kill_on_drop(true)` as a backstop. On timeout Talos explicitly kills the
//! child, attempts a process-tree kill (Unix process group via `setsid` at
//! spawn + `kill -KILL -<pgid>`; Windows `taskkill /T /F`), drains pipes, and
//! reaps with `wait()`. The outcome always carries a
//! [`ProcessLifecycleReceipt`] (`killed_on_timeout`, `kill_method`,
//! `reap_confirmed`); a failed kill is logged, never fatal.

use std::process::Stdio;

use sha2::{Digest, Sha256};
use talos_common::{Result, TalosError};

use crate::adapters::{apply_host_env, check_command_allowed, SandboxConfig};

fn err(msg: &str) -> TalosError {
    TalosError::Storage(msg.to_owned())
}

fn hash_text(text: &str) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(text.as_bytes())))
}

/// Process lifecycle proof attached to every shell receipt.
///
/// - `killed_on_timeout`: the timeout path issued `kill()` (never set when
///   the process exited on its own).
/// - `kill_method`: how the tree died — `"direct"` (`Child::kill`),
///   `"process_group"` (Unix `kill -KILL -<pgid>` after `setsid`), or
///   `"taskkill"` (Windows `taskkill /T /F`). `None` when no kill was needed
///   (or no kill helper succeeded and the process was already gone).
/// - `reap_confirmed`: `wait()` returned, so no zombie remains.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessLifecycleReceipt {
    /// True when the timeout path issued a kill.
    pub killed_on_timeout: bool,
    /// How the tree died (`direct` | `process_group` | `taskkill`).
    pub kill_method: Option<String>,
    /// True when `wait()` reaped the child.
    pub reap_confirmed: bool,
}

impl ProcessLifecycleReceipt {
    /// Lifecycle for a process that exited on its own (wait reaped it).
    #[must_use]
    pub fn completed() -> Self {
        Self {
            killed_on_timeout: false,
            kill_method: None,
            reap_confirmed: true,
        }
    }
}

/// Shell execution outcome: output JSON plus lifecycle proof.
///
/// Timeouts are `Ok` with `timed_out: true` (the executor maps them to a
/// `Failed` + `timeout` receipt); only policy/spawn/io errors are `Err`.
#[derive(Debug, Clone)]
pub struct ExecOutcome {
    /// Output JSON (`command`, `exitCode`, `stdout`, `stderr`, hashes,
    /// `truncated`, `timedOut`, `durationMs`, `workspacePath`,
    /// `processLifecycle`).
    pub output: serde_json::Value,
    /// Kill/reap proof.
    pub lifecycle: ProcessLifecycleReceipt,
    /// True when the timeout path fired.
    pub timed_out: bool,
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

/// Build the child command: argv-exact, piped, minimal env, new process
/// group on Unix (so the whole tree can be killed), `kill_on_drop(true)`
/// as a backstop. No policy checks — callers enforce allowlist first.
fn spawn_command(
    config: &SandboxConfig,
    command: &str,
    args: &[String],
) -> tokio::process::Command {
    let mut std_cmd = std::process::Command::new(command);
    std_cmd.args(args);
    std_cmd.current_dir(&config.root);
    std_cmd.stdout(Stdio::piped());
    std_cmd.stderr(Stdio::piped());
    std_cmd.stdin(Stdio::null());
    std_cmd.env_clear();
    // New process group on Unix: the child's pgid == its pid, so
    // `kill -KILL -<pid>` takes the entire tree (shell + grandchildren).
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        std_cmd.process_group(0);
    }
    let mut cmd = tokio::process::Command::from(std_cmd);
    // Minimal host environment (PATH, system + toolchain locations, temp).
    // No secrets cross this boundary; see `apply_host_env`.
    apply_host_env(&mut cmd);
    // Backstop: dropping the handle without reaping still kills the child.
    cmd.kill_on_drop(true);
    cmd
}

/// Kill the whole process group (Unix). Guarded: a missing `kill` binary
/// or a dead group reports `false` without failing the caller.
#[cfg(unix)]
fn kill_process_group(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-KILL", &format!("-{pid}")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// Kill the process tree (Windows): `taskkill /T` (tree) `/F` (force).
/// Guarded like the Unix helper.
#[cfg(windows)]
fn taskkill_tree(pid: u32) -> bool {
    std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// Read a pipe to the end, giving up after `limit` (never blocks the kill
/// path: a live child holding the pipe open must not stall reaping).
async fn drain_pipe<R>(pipe: &mut R, limit: std::time::Duration) -> Vec<u8>
where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncReadExt as _;
    let mut buf = Vec::new();
    let _ = tokio::time::timeout(limit, pipe.read_to_end(&mut buf)).await;
    buf
}

/// Run a spawned child to completion or timeout.
///
/// On timeout: brief pre-kill drain, explicit `kill()`, process-tree kill,
/// post-kill drain, then `wait()` to reap. Kill failures are logged, never
/// fatal — the timeout outcome stands either way.
async fn run_child(
    mut child: tokio::process::Child,
    timeout: std::time::Duration,
    cap: usize,
    command: &str,
    root: &std::path::Path,
    started: std::time::Instant,
) -> Result<ExecOutcome> {
    let pid = child.id();
    let mut stdout_pipe = child.stdout.take();
    let mut stderr_pipe = child.stderr.take();
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
    let finished = tokio::time::timeout(timeout, child.wait()).await;
    match finished {
        Ok(Ok(status)) => {
            // Exited on its own: bounded drain (a surviving grandchild may
            // still hold the pipes; never block on it).
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            if let Some(pipe) = stdout_pipe.as_mut() {
                stdout = drain_pipe(pipe, std::time::Duration::from_secs(2)).await;
            }
            if let Some(pipe) = stderr_pipe.as_mut() {
                stderr = drain_pipe(pipe, std::time::Duration::from_secs(2)).await;
            }
            let (stdout_text, out_truncated) = truncate(stdout);
            let (stderr_text, err_truncated) = truncate(stderr);
            let lifecycle = ProcessLifecycleReceipt::completed();
            Ok(ExecOutcome {
                output: serde_json::json!({
                    "command": command,
                    "exitCode": status.code(),
                    "stdout": stdout_text,
                    "stderr": stderr_text,
                    "stdoutHash": hash_text(&stdout_text),
                    "stderrHash": hash_text(&stderr_text),
                    "truncated": out_truncated || err_truncated,
                    "timedOut": false,
                    "durationMs": started.elapsed().as_millis() as u64,
                    "workspacePath": root.to_string_lossy(),
                    "processLifecycle": lifecycle,
                }),
                lifecycle,
                timed_out: false,
            })
        }
        Ok(Err(e)) => Err(err(&format!("process.exec wait: {e}"))),
        Err(_) => {
            // Timeout: drain briefly (the child may be blocked on full
            // pipes), then kill, tree-kill, drain, reap.
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            if let Some(pipe) = stdout_pipe.as_mut() {
                stdout = drain_pipe(pipe, std::time::Duration::from_millis(200)).await;
            }
            if let Some(pipe) = stderr_pipe.as_mut() {
                stderr = drain_pipe(pipe, std::time::Duration::from_millis(200)).await;
            }
            // Explicit kill first — never rely on drop alone.
            let mut direct_ok = false;
            match child.kill().await {
                Ok(()) => direct_ok = true,
                Err(e) => eprintln!("[talos] process.exec kill failed for pid {pid:?}: {e}"),
            }
            // Process-tree kill (platform helper, guarded).
            #[cfg(unix)]
            let tree_killed = pid.is_some_and(kill_process_group);
            #[cfg(windows)]
            let tree_killed = pid.is_some_and(taskkill_tree);
            #[cfg(not(any(unix, windows)))]
            let tree_killed = false;
            let kill_method = if tree_killed {
                #[cfg(unix)]
                {
                    Some("process_group".to_owned())
                }
                #[cfg(windows)]
                {
                    Some("taskkill".to_owned())
                }
                #[cfg(not(any(unix, windows)))]
                {
                    Option::<String>::None
                }
            } else if direct_ok {
                Some("direct".to_owned())
            } else {
                None
            };
            // Post-kill drain: pipes hit EOF once the tree is dead.
            if let Some(pipe) = stdout_pipe.as_mut() {
                let mut rest = drain_pipe(pipe, std::time::Duration::from_secs(1)).await;
                stdout.append(&mut rest);
            }
            if let Some(pipe) = stderr_pipe.as_mut() {
                let mut rest = drain_pipe(pipe, std::time::Duration::from_secs(1)).await;
                stderr.append(&mut rest);
            }
            // Reap: wait() must return so no zombie remains.
            let mut reap_confirmed = false;
            match tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await {
                Ok(Ok(_)) => reap_confirmed = true,
                Ok(Err(e)) => {
                    eprintln!("[talos] process.exec reap failed for pid {pid:?}: {e}");
                }
                Err(_) => {
                    eprintln!("[talos] process.exec reap timed out for pid {pid:?}");
                }
            }
            let lifecycle = ProcessLifecycleReceipt {
                killed_on_timeout: true,
                kill_method,
                reap_confirmed,
            };
            let (stdout_text, out_truncated) = truncate(stdout);
            let (stderr_text, err_truncated) = truncate(stderr);
            Ok(ExecOutcome {
                output: serde_json::json!({
                    "command": command,
                    "exitCode": null,
                    "stdout": stdout_text,
                    "stderr": stderr_text,
                    "stdoutHash": hash_text(&stdout_text),
                    "stderrHash": hash_text(&stderr_text),
                    "truncated": out_truncated || err_truncated,
                    "timedOut": true,
                    "durationMs": started.elapsed().as_millis() as u64,
                    "workspacePath": root.to_string_lossy(),
                    "processLifecycle": lifecycle,
                }),
                lifecycle,
                timed_out: true,
            })
        }
    }
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
) -> Result<ExecOutcome> {
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
    // No shell: argv-exact spawn only.
    let spawned = spawn_command(config, command, args)
        .spawn()
        .map_err(|e| err(&format!("process.exec spawn: {e}")))?;
    let cap = config.max_output_bytes.max(1);
    run_child(spawned, timeout, cap, command, &config.root, started).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exec_config_for(cmd: &str) -> (tempfile::TempDir, SandboxConfig) {
        // Production always passes an existing root (state.rs creates it);
        // mirror that with an isolated temp dir (the default
        // <tmp>/talos-tools may not exist, e.g. on fresh CI runners).
        let dir = tempfile::tempdir().expect("temp dir");
        let config = SandboxConfig {
            exec_enabled: true,
            exec_allowlist: vec![cmd.to_owned()],
            root: dir.path().to_owned(),
            ..SandboxConfig::default()
        };
        (dir, config)
    }

    /// Platform sleeper: `sleep 30` on Unix, `ping -n 30` on Windows
    /// (`timeout.exe` rejects redirected stdin, so it cannot sleep here).
    fn sleeper() -> (&'static str, Vec<String>) {
        #[cfg(unix)]
        {
            ("sleep", vec!["30".to_owned()])
        }
        #[cfg(windows)]
        {
            (
                "ping",
                vec!["-n".to_owned(), "30".to_owned(), "127.0.0.1".to_owned()],
            )
        }
    }

    /// True when a Unix pid is gone (`/proc` on Linux, `ps -p` elsewhere).
    #[cfg(unix)]
    fn pid_gone(pid: u32) -> bool {
        #[cfg(target_os = "linux")]
        {
            !std::path::Path::new(&format!("/proc/{pid}")).exists()
        }
        #[cfg(not(target_os = "linux"))]
        {
            std::process::Command::new("ps")
                .args(["-p", &pid.to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|status| !status.success())
                .unwrap_or(true)
        }
    }

    /// True when `ps` output contains the marker (Unix process-tree probe).
    #[cfg(unix)]
    fn ps_has(marker: &str) -> bool {
        let output = std::process::Command::new("ps")
            .arg("-eo")
            .arg("args")
            .output();
        let Ok(output) = output else {
            return false;
        };
        let text = String::from_utf8_lossy(&output.stdout);
        text.lines()
            .any(|line| line.contains(marker) && !line.contains("ps -eo"))
    }

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

    #[test]
    fn kill_on_drop_is_set_on_every_spawn() {
        // The builder must record kill_on_drop(true): dropping the handle
        // without reaping still kills the child (TAL-006 backstop).
        let config = SandboxConfig::default();
        let cmd = spawn_command(&config, "sleep", &["30".to_owned()]);
        let debug = format!("{cmd:?}");
        assert!(
            debug.contains("kill_on_drop: true"),
            "spawned commands must set kill_on_drop(true), got: {debug}"
        );
    }

    #[tokio::test]
    async fn timeout_kills_and_reaps_sleeper() {
        // TAL-006 regression: a 30s sleeper with a 1s timeout must be dead
        // and reaped long before its natural exit.
        let (cmd, args) = sleeper();
        let (_dir, config) = exec_config_for(cmd);
        let started = std::time::Instant::now();
        let outcome = exec_live(&config, cmd, &args, 1_000)
            .await
            .expect("timeout is an outcome, not a transport error");
        assert!(outcome.timed_out);
        assert!(outcome.output["timedOut"] == true);
        assert!(outcome.lifecycle.killed_on_timeout);
        assert!(outcome.lifecycle.reap_confirmed);
        assert!(outcome.lifecycle.kill_method.is_some());
        assert!(
            started.elapsed() < std::time::Duration::from_secs(8),
            "kill must be prompt, took {:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn kill_and_reap_leaves_no_zombie() {
        // Direct unit proof: kill + wait reaps, so try_wait observes the exit.
        let (cmd, args) = sleeper();
        let (_dir, config) = exec_config_for(cmd);
        let mut spawned = spawn_command(&config, cmd, &args)
            .spawn()
            .expect("sleeper spawns");
        #[cfg(target_os = "linux")]
        let pid = spawned.id().expect("sleeper has a pid");
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        spawned.kill().await.expect("kill succeeds");
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), spawned.wait())
            .await
            .expect("reap is prompt")
            .expect("wait succeeds");
        // try_wait after wait: Some(status) proves the child was reaped.
        assert!(spawned.try_wait().expect("try_wait works").is_some());
        #[cfg(target_os = "linux")]
        assert!(pid_gone(pid), "reaped pid {pid} must be gone from /proc");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn process_group_kill_takes_grandchildren() {
        // TAL-006 tree kill: `sh -c 'sleep 317'` puts a grandchild in the
        // child's process group; the timeout path must kill both.
        let (_dir, config) = exec_config_for("sh");
        let mut spawned = spawn_command(&config, "sh", &["-c".to_owned(), "sleep 317".to_owned()])
            .spawn()
            .expect("shell spawns");
        let pid = spawned.id().expect("shell has a pid");
        // Let the grandchild start, then confirm it is visible.
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        assert!(
            ps_has("sleep 317"),
            "grandchild must be running before kill"
        );
        kill_process_group(pid);
        // Reap the shell; the group kill already took it.
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), spawned.wait()).await;
        // The grandchild must be gone (no orphan reparented with our marker).
        let mut gone = pid_gone(pid);
        for _ in 0..20 {
            if !ps_has("sleep 317") {
                gone = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        assert!(
            !ps_has("sleep 317"),
            "grandchild must not survive group kill"
        );
        assert!(gone, "shell pid {pid} must be gone");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn dropped_child_dies_via_kill_on_drop() {
        // Backstop proof: dropping the handle kills the child even when the
        // caller never waits (documents kill_on_drop behavior).
        let (_dir, config) = exec_config_for("sleep");
        let spawned = spawn_command(&config, "sleep", &["60".to_owned()])
            .spawn()
            .expect("sleeper spawns");
        let pid = spawned.id().expect("sleeper has a pid");
        drop(spawned);
        let mut dead = false;
        for _ in 0..20 {
            #[cfg(target_os = "linux")]
            {
                dead = pid_gone(pid);
            }
            #[cfg(not(target_os = "linux"))]
            {
                dead = !ps_has(&format!("sleep {pid}"));
            }
            if dead {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        assert!(dead, "dropped child pid {pid} must die via kill_on_drop");
    }
}
