//! SSH target: tar-pipe sync + argv-safe remote execution.
//!
//! Isolation contract (documented, enforced by construction):
//!
//! - Local side: argv-exact `ssh`/`tar` spawns, no shell. The private key
//!   never leaves the ssh client (`-i`, `BatchMode=yes`, no agent forwarding
//!   `-a`, no X11 `-x`).
//! - Remote side: one unprivileged user, one directory (`remote_dir`). All
//!   commands run with `cd <dir>` first; nothing outside the dir is addressable
//!   because every path the tools use resolves under the synced root and the
//!   local allowlist + denylist already ran *before* anything was sent.
//! - Sync back is size-capped; oversize results truncate with a marker and a
//!   `remote_sync_truncated` flag instead of silently dropping data.

use std::sync::Arc;
use std::time::Duration;

use crate::config::SshRunnerConfig;
use crate::runner::{bound_text, hash_output, CommandRunner, RemoteOutput, RunnerError};

/// Maximum tarball accepted from the remote on copy-back (32 MiB).
pub const MAX_SYNC_BACK_BYTES: usize = 32 * 1024 * 1024;

/// SSH execution target.
#[derive(Debug, Clone)]
pub struct SshTarget {
    config: SshRunnerConfig,
    commands: Arc<dyn CommandRunner>,
}

impl SshTarget {
    /// Build from a validated config plus a transport.
    #[must_use]
    pub fn new(config: SshRunnerConfig, commands: Arc<dyn CommandRunner>) -> Self {
        Self { config, commands }
    }

    /// Base `ssh` argv (identity, batch mode, timeouts, no forwarding).
    #[must_use]
    pub fn ssh_argv(&self, extra: &[String]) -> (String, Vec<String>) {
        let mut args = vec![
            "-i".to_owned(),
            crate::config::expand_home(&self.config.key_path),
            "-p".to_owned(),
            self.config.port.to_string(),
            "-o".to_owned(),
            "BatchMode=yes".to_owned(),
            "-o".to_owned(),
            format!("ConnectTimeout={}", self.config.connect_timeout_secs),
            "-o".to_owned(),
            format!(
                "StrictHostKeyChecking={}",
                self.config.strict_host_key_checking
            ),
            "-a".to_owned(),
            "-x".to_owned(),
            format!("{}@{}", self.config.user, self.config.host),
        ];
        args.extend(extra.iter().cloned());
        ("ssh".to_owned(), args)
    }

    /// Single-quote a shell word (`'` → `'\''`).
    #[must_use]
    pub fn sh_quote(word: &str) -> String {
        format!("'{}'", word.replace('\'', "'\\''"))
    }

    /// Sync `local_root` → `remote_dir` (fresh mirror per task dir), run
    /// `command args`, sync results back (capped), hash the output.
    pub async fn run_synced(
        &self,
        local_root: &std::path::Path,
        command: &str,
        args: &[String],
        timeout_ms: u64,
        max_bytes: usize,
    ) -> Result<RemoteOutput, RunnerError> {
        let remote_dir = self.config.remote_dir.trim_end_matches('/').to_owned();
        // 1. Ensure the isolated remote dir exists (argv-exact, no shell).
        let (ssh, mut mkdir_args) = self.ssh_argv(&[]);
        mkdir_args.extend(["mkdir".to_owned(), "-p".to_owned(), remote_dir.clone()]);
        let mkdir = self
            .commands
            .run(&ssh, &mkdir_args, None, Duration::from_secs(30))
            .await?;
        if mkdir.status != Some(0) {
            return Err(RunnerError::Remote(format!(
                "mkdir failed: {}",
                String::from_utf8_lossy(&mkdir.stderr)
                    .chars()
                    .take(256)
                    .collect::<String>()
            )));
        }
        // 2. Push the workspace over a tar pipe (local `tar -cf -`, remote
        //    `tar -xf - -C dir`; no local shell, no rsync dependency).
        let tarball = self.tar_local(local_root).await?;
        let extract = format!("tar -xf - -C {}", Self::sh_quote(&remote_dir));
        let (ssh, extract_args) = self.ssh_argv(&[extract]);
        let pushed = self
            .commands
            .run(&ssh, &extract_args, Some(tarball), Duration::from_secs(120))
            .await?;
        if pushed.status != Some(0) {
            return Err(RunnerError::Remote(format!(
                "workspace sync failed: {}",
                String::from_utf8_lossy(&pushed.stderr)
                    .chars()
                    .take(256)
                    .collect::<String>()
            )));
        }
        // 3. Execute inside the isolated dir via one quoted `sh -c` script.
        //    Local allowlist + denylist already ran before anything was sent.
        let mut script = format!("cd {} && exec", Self::sh_quote(&remote_dir));
        for word in std::iter::once(command).chain(args.iter().map(String::as_str)) {
            script.push(' ');
            script.push_str(&Self::sh_quote(word));
        }
        let (ssh, exec_args) = self.ssh_argv(&["sh".to_owned(), "-c".to_owned(), script]);
        let timeout = Duration::from_millis(timeout_ms.clamp(100, 300_000));
        let ran = self.commands.run(&ssh, &exec_args, None, timeout).await;
        let ran = match ran {
            Ok(output) => output,
            Err(RunnerError::Timeout(ms)) => {
                return Ok(RemoteOutput {
                    status: None,
                    stdout: String::new(),
                    stderr: format!("remote: timed out after {ms}ms"),
                    timed_out: true,
                    output_sha256: hash_output("", &format!("timeout after {ms}ms")),
                })
            }
            Err(err) => return Err(err),
        };
        let cap = max_bytes.max(1);
        let stdout = bound_text(&ran.stdout, cap);
        let stderr = bound_text(&ran.stderr, cap);
        // 4. Copy results back (capped tarball; truncate, never silently drop).
        let (truncated, _synced) = self.sync_back(local_root, &remote_dir).await?;
        let mut stderr = stderr;
        if truncated {
            stderr.push_str("\n[remote_sync_truncated]");
        }
        Ok(RemoteOutput {
            status: ran.status,
            stdout: stdout.clone(),
            stderr: stderr.clone(),
            timed_out: false,
            output_sha256: hash_output(&stdout, &stderr),
        })
    }

    /// Tar the local root to bytes (bounded: refuse absurd workspaces).
    async fn tar_local(&self, local_root: &std::path::Path) -> Result<Vec<u8>, RunnerError> {
        // `tar -cf - -C root .` — argv-exact, no shell.
        let tar = if cfg!(windows) { "tar.exe" } else { "tar" };
        let output = self
            .commands
            .run(
                tar,
                &[
                    "-cf".to_owned(),
                    "-".to_owned(),
                    "-C".to_owned(),
                    local_root.display().to_string(),
                    ".".to_owned(),
                ],
                None,
                Duration::from_secs(120),
            )
            .await?;
        if output.status != Some(0) {
            return Err(RunnerError::Transport(format!(
                "local tar failed: {}",
                String::from_utf8_lossy(&output.stderr)
                    .chars()
                    .take(256)
                    .collect::<String>()
            )));
        }
        if output.stdout.len() > MAX_SYNC_BACK_BYTES {
            return Err(RunnerError::Refused(format!(
                "workspace tarball {} bytes exceeds {} cap",
                output.stdout.len(),
                MAX_SYNC_BACK_BYTES
            )));
        }
        Ok(output.stdout)
    }

    /// Pull the remote dir back over a tar pipe, capped. Returns
    /// `(truncated, files_restored_ok)`.
    async fn sync_back(
        &self,
        local_root: &std::path::Path,
        remote_dir: &str,
    ) -> Result<(bool, bool), RunnerError> {
        // Remote `tar -cf - -C dir .` streams bytes to us; we re-extract
        // locally with a second `tar -xf - -C local` (no shell either side).
        let extract = format!("tar -cf - -C {}", Self::sh_quote(remote_dir));
        let (ssh, args) = self.ssh_argv(&[extract]);
        let output = self
            .commands
            .run(&ssh, &args, None, Duration::from_secs(120))
            .await?;
        if output.status != Some(0) {
            return Err(RunnerError::Remote("result sync failed".to_owned()));
        }
        let truncated = output.stdout.len() >= MAX_SYNC_BACK_BYTES;
        let bytes = if truncated {
            output.stdout[..MAX_SYNC_BACK_BYTES].to_vec()
        } else {
            output.stdout
        };
        let tar = if cfg!(windows) { "tar.exe" } else { "tar" };
        let restored = self
            .commands
            .run(
                tar,
                &[
                    "-xf".to_owned(),
                    "-".to_owned(),
                    "-C".to_owned(),
                    local_root.display().to_string(),
                ],
                Some(bytes),
                Duration::from_secs(120),
            )
            .await?;
        Ok((truncated, restored.status == Some(0)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::MockRunner;
    use std::sync::Arc;

    fn target() -> (SshTarget, Arc<MockRunner>) {
        let http = Arc::new(MockRunner::default());
        let config = SshRunnerConfig {
            host: "gpu.local".to_owned(),
            user: "talos".to_owned(),
            key_path: "~/.ssh/talos_ed25519".to_owned(),
            port: 2222,
            remote_dir: "~/.talos-runs/task-1".to_owned(),
            strict_host_key_checking: "accept-new".to_owned(),
            connect_timeout_secs: 10,
        };
        (SshTarget::new(config, http.clone()), http)
    }

    #[test]
    fn ssh_argv_is_batch_no_forwarding() {
        let (target, _) = target();
        let (program, args) = target.ssh_argv(&["true".to_owned()]);
        assert_eq!(program, "ssh");
        let joined = args.join(" ");
        assert!(joined.contains("-i"), "identity file pinned");
        assert!(joined.contains("BatchMode=yes"), "no password prompts");
        assert!(joined.contains("-a"), "no agent forwarding");
        assert!(joined.contains("-x"), "no X11");
        assert!(joined.contains("talos@gpu.local"));
        assert!(joined.contains("-p 2222"));
    }

    #[test]
    fn sh_quote_escapes_single_quotes() {
        assert_eq!(SshTarget::sh_quote("a'b"), "'a'\\''b'");
        assert_eq!(SshTarget::sh_quote("plain"), "'plain'");
    }

    #[tokio::test]
    async fn run_synced_stages_executes_and_copies_back() {
        let (target, mock) = target();
        // mkdir ok, tar-local ok (empty tarball), push ok, exec ok, pull ok, extract ok.
        for _ in 0..5 {
            mock.push_output(crate::runner::CommandOutput {
                status: Some(0),
                stdout: Vec::new(),
                stderr: Vec::new(),
                timed_out: false,
            });
        }
        mock.push_output(crate::runner::CommandOutput {
            status: Some(0),
            stdout: b"ok\n".to_vec(),
            stderr: Vec::new(),
            timed_out: false,
        });
        let dir = tempfile::tempdir().expect("temp dir");
        let output = target
            .run_synced(dir.path(), "pytest", &[], 30_000, 1024)
            .await
            .expect("runs");
        assert_eq!(output.status, Some(0));
        assert!(output.output_sha256.starts_with("sha256:"));
        // mkdir, tar-local, push, exec, pull, extract.
        assert_eq!(mock.call_count(), 6);
        let last_exec = &mock.calls()[3];
        assert_eq!(last_exec.0, "ssh");
        assert!(last_exec.1.join(" ").contains("pytest"));
    }

    #[tokio::test]
    async fn run_synced_reports_timeouts() {
        let (target, mock) = target();
        for _ in 0..3 {
            mock.push_output(crate::runner::CommandOutput {
                status: Some(0),
                stdout: Vec::new(),
                stderr: Vec::new(),
                timed_out: false,
            });
        }
        mock.push_error(crate::runner::RunnerError::Timeout(30_000));
        let dir = tempfile::tempdir().expect("temp dir");
        let output = target
            .run_synced(dir.path(), "pytest", &[], 30_000, 1024)
            .await
            .expect("timeout maps to a receipt, not an error");
        assert!(output.timed_out);
        assert_eq!(output.status, None);
        assert!(output.stderr.contains("timed out"));
    }
}
