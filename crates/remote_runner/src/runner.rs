//! Command transport: argv-exact local process spawning plus a scripted
//! mock. No shells on the calling side, ever — remote shells (ssh multiplex,
//! `docker run` args) only ever receive allowlisted, denylist-checked argv
//! that the policy broker already approved.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;

/// Transport failures (timeouts, spawn errors, protocol violations).
#[derive(Debug, Clone, thiserror::Error)]
pub enum RunnerError {
    /// Spawning or I/O failed.
    #[error("transport: {0}")]
    Transport(String),
    /// The command exceeded its timeout (best-effort kill issued).
    #[error("timeout after {0}ms")]
    Timeout(u64),
    /// The remote side refused or misbehaved.
    #[error("remote: {0}")]
    Remote(String),
    /// Refusing to run (missing binary, bad config, oversize payload).
    #[error("refused: {0}")]
    Refused(String),
}

impl From<RunnerError> for talos_common::TalosError {
    fn from(err: RunnerError) -> Self {
        talos_common::TalosError::Internal(format!("remote runner: {err}"))
    }
}

/// Raw process result.
#[derive(Debug, Clone)]
pub struct CommandOutput {
    /// Exit status (`None` = killed/signalled).
    pub status: Option<i32>,
    /// Bounded stdout bytes.
    pub stdout: Vec<u8>,
    /// Bounded stderr bytes.
    pub stderr: Vec<u8>,
    /// True when the timeout fired first.
    pub timed_out: bool,
}

/// Bounded remote execution result, ready for the evidence ledger.
#[derive(Debug, Clone)]
pub struct RemoteOutput {
    /// Exit status (`None` = killed/signalled).
    pub status: Option<i32>,
    /// Bounded stdout (UTF-8 lossy).
    pub stdout: String,
    /// Bounded stderr (UTF-8 lossy).
    pub stderr: String,
    /// True when the timeout fired first.
    pub timed_out: bool,
    /// `sha256:` over `stdout\nstderr` (integrity hash for the ledger).
    pub output_sha256: String,
}

/// Spawn `program args` with optional piped stdin and a kill-on-timeout.
#[async_trait]
pub trait CommandRunner: Send + Sync + std::fmt::Debug {
    /// Run and wait, capturing both streams (callers cap sizes downstream).
    async fn run(
        &self,
        program: &str,
        args: &[String],
        stdin: Option<Vec<u8>>,
        timeout: Duration,
    ) -> Result<CommandOutput, RunnerError>;
}

/// Live transport: `tokio::process` with argv-exact spawn and timeout kill.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemRunner;

impl SystemRunner {
    /// Shared handle (the runner holds no state).
    #[must_use]
    pub fn shared() -> Arc<dyn CommandRunner> {
        Arc::new(Self)
    }
}

#[async_trait]
impl CommandRunner for SystemRunner {
    async fn run(
        &self,
        program: &str,
        args: &[String],
        stdin: Option<Vec<u8>>,
        timeout: Duration,
    ) -> Result<CommandOutput, RunnerError> {
        let mut cmd = tokio::process::Command::new(program);
        cmd.args(args)
            .stdin(if stdin.is_some() {
                std::process::Stdio::piped()
            } else {
                std::process::Stdio::null()
            })
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .env_clear();
        // Minimal locale + Windows system root only: no secrets, no home, no
        // shell init cross over. SystemRoot is required for CryptoAPI-backed
        // children (ssh, PowerShell) to start at all.
        cmd.env("LANG", "C.UTF-8");
        if let Some(root) = std::env::var_os("SystemRoot") {
            cmd.env("SystemRoot", root);
        }
        // TAL-006 discipline (mirrors tool_runtime): kill_on_drop backstop,
        // explicit kill + reap on timeout, bounded post-exit pipe drain.
        cmd.kill_on_drop(true);
        let mut child = cmd
            .spawn()
            .map_err(|err| RunnerError::Transport(format!("spawn {program}: {err}")))?;
        if let Some(input) = stdin {
            if let Some(mut pipe) = child.stdin.take() {
                use tokio::io::AsyncWriteExt as _;
                pipe.write_all(&input)
                    .await
                    .map_err(|err| RunnerError::Transport(format!("stdin: {err}")))?;
                drop(pipe);
            }
        }
        let mut stdout = child.stdout.take();
        let mut stderr = child.stderr.take();
        let timeout_ms = timeout.as_millis() as u64;
        let timed_out = tokio::select! {
            result = child.wait() => {
                result.map_err(|err| RunnerError::Transport(format!("wait: {err}")))?;
                false
            }
            () = tokio::time::sleep(timeout) => {
                // Explicit kill first — never rely on drop alone — then reap.
                let _ = child.kill().await;
                let _ = child.wait().await;
                true
            }
        };
        if timed_out {
            return Err(RunnerError::Timeout(timeout_ms));
        }
        // Post-exit drain: pipes hit EOF once the child is reaped.
        let mut out_bytes = Vec::new();
        let mut err_bytes = Vec::new();
        if let Some(pipe) = stdout.as_mut() {
            use tokio::io::AsyncReadExt as _;
            let _ = tokio::time::timeout(Duration::from_secs(5), pipe.read_to_end(&mut out_bytes))
                .await;
        }
        if let Some(pipe) = stderr.as_mut() {
            use tokio::io::AsyncReadExt as _;
            let _ = tokio::time::timeout(Duration::from_secs(5), pipe.read_to_end(&mut err_bytes))
                .await;
        }
        Ok(CommandOutput {
            status: child
                .try_wait()
                .ok()
                .flatten()
                .and_then(|status| status.code()),
            stdout: out_bytes,
            stderr: err_bytes,
            timed_out: false,
        })
    }
}

/// Scripted transport for tests: queued outputs, recorded calls (program,
/// args, stdin byte length). Never touches the network or disk.
#[derive(Debug, Default)]
pub struct MockRunner {
    outputs: Mutex<Vec<Result<CommandOutput, RunnerError>>>,
    calls: Mutex<Vec<(String, Vec<String>, usize)>>,
}

impl MockRunner {
    /// Queue one scripted output (consumed FIFO per `run`).
    pub fn push_output(&self, output: CommandOutput) {
        self.outputs.lock().expect("mock").push(Ok(output));
    }

    /// Queue a scripted transport error (timeout, refused, ...).
    pub fn push_error(&self, err: RunnerError) {
        self.outputs.lock().expect("mock").push(Err(err));
    }

    /// Queue a successful output with literal stdout/stderr.
    pub fn push_success(&self, stdout: &str, stderr: &str) {
        self.push_output(CommandOutput {
            status: Some(0),
            stdout: stdout.as_bytes().to_vec(),
            stderr: stderr.as_bytes().to_vec(),
            timed_out: false,
        });
    }

    /// Observed calls for assertions.
    pub fn calls(&self) -> Vec<(String, Vec<String>, usize)> {
        self.calls.lock().expect("mock").clone()
    }

    /// Number of observed calls.
    pub fn call_count(&self) -> usize {
        self.calls.lock().expect("mock").len()
    }

    /// Shared handle for runner construction.
    #[must_use]
    pub fn shared(self: &Arc<Self>) -> Arc<dyn CommandRunner> {
        self.clone()
    }
}

#[async_trait]
impl CommandRunner for MockRunner {
    async fn run(
        &self,
        program: &str,
        args: &[String],
        stdin: Option<Vec<u8>>,
        _timeout: Duration,
    ) -> Result<CommandOutput, RunnerError> {
        self.calls.lock().expect("mock").push((
            program.to_owned(),
            args.to_vec(),
            stdin.as_ref().map_or(0, Vec::len),
        ));
        let mut outputs = self.outputs.lock().expect("mock");
        if outputs.is_empty() {
            return Err(RunnerError::Transport("no mock output queued".to_owned()));
        }
        outputs.remove(0)
    }
}

/// Cap bytes and render lossy text for ledger output.
#[must_use]
pub fn bound_text(bytes: &[u8], cap: usize) -> String {
    const MARKER: &str = "\n[…truncated…]";
    let text = String::from_utf8_lossy(bytes);
    if text.len() <= cap {
        return text.into_owned();
    }
    let mut out = text.chars().take(cap).collect::<String>();
    out.push_str(MARKER);
    out
}

/// `sha256:` over stdout, a newline, then stderr.
#[must_use]
pub fn hash_output(stdout: &str, stderr: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(stdout.as_bytes());
    hasher.update(b"\n");
    hasher.update(stderr.as_bytes());
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bound_text_truncates_with_marker() {
        assert_eq!(bound_text(b"hello", 100), "hello");
        let capped = bound_text(b"abcdef", 3);
        assert!(capped.contains("[…truncated…]"));
        assert!(capped.starts_with("abc"));
    }

    #[test]
    fn hash_output_is_stable_and_sized() {
        let first = hash_output("out", "err");
        assert_eq!(first, hash_output("out", "err"));
        assert!(first.starts_with("sha256:"));
        assert_eq!(first.len(), "sha256:".len() + 64);
        assert_ne!(first, hash_output("out", "other"));
    }

    #[tokio::test]
    async fn system_runner_runs_true_argv_exact() {
        let runner = SystemRunner;
        #[cfg(unix)]
        let (program, args) = ("true", Vec::new());
        #[cfg(windows)]
        let (program, args) = ("cmd", vec!["/C".to_owned(), "exit 0".to_owned()]);
        let output = runner
            .run(program, &args, None, Duration::from_secs(10))
            .await
            .expect("trivial command runs");
        assert_eq!(output.status, Some(0));
        assert!(!output.timed_out);
    }

    #[tokio::test]
    async fn system_runner_times_out_and_reports() {
        let runner = SystemRunner;
        #[cfg(unix)]
        let (program, args) = ("sleep", vec!["30".to_owned()]);
        #[cfg(windows)]
        // cmd-internal busy loop: no external binary, no PATH needed.
        let (program, args) = (
            "cmd",
            vec![
                "/C".to_owned(),
                "for /L %i in (1,1,200000000) do rem".to_owned(),
            ],
        );
        let err = runner
            .run(program, &args, None, Duration::from_millis(200))
            .await
            .expect_err("must time out");
        assert!(matches!(err, RunnerError::Timeout(_)), "got {err:?}");
    }

    #[tokio::test]
    async fn mock_runner_scripts_and_records() {
        let runner = Arc::new(MockRunner::default());
        runner.push_success("hi", "");
        let output = runner
            .run(
                "ssh",
                &["x".to_owned()],
                Some(vec![1, 2, 3]),
                Duration::from_secs(1),
            )
            .await
            .expect("scripted");
        assert_eq!(output.status, Some(0));
        assert_eq!(runner.call_count(), 1);
        assert_eq!(runner.calls()[0].2, 3, "stdin length recorded");
    }
}
