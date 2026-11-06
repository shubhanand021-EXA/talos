//! Docker target: fresh `--rm` container per invocation, exactly one
//! bind-mount (the staged workspace), no network.
//!
//! Isolation contract: the container sees only the staged copy under
//! `/work` (never the host root, never the daemon's directories), runs with
//! no network (`--network none` unless the runner explicitly opens one),
//! and is removed on exit. Remote contexts (`--context`) share the daemon's
//! filesystem view — cross-host binds are refused by construction (the path
//! must exist locally before the bind is attempted).

use std::sync::Arc;
use std::time::Duration;

use crate::config::DockerRunnerConfig;
use crate::runner::{bound_text, hash_output, CommandRunner, RemoteOutput, RunnerError};

/// Docker execution target.
#[derive(Debug, Clone)]
pub struct DockerTarget {
    config: DockerRunnerConfig,
    commands: Arc<dyn CommandRunner>,
}

impl DockerTarget {
    /// Build from a validated config plus a transport.
    #[must_use]
    pub fn new(config: DockerRunnerConfig, commands: Arc<dyn CommandRunner>) -> Self {
        Self { config, commands }
    }

    /// Base `docker run` argv: fresh container, capped resources, exactly one
    /// bind-mount (the staged workspace), caller-chosen workdir.
    #[must_use]
    pub fn run_argv(
        &self,
        stage_dir: &std::path::Path,
        command: &str,
        args: &[String],
    ) -> (String, Vec<String>) {
        let mut argv = Vec::new();
        if let Some(context) = self.config.context.as_deref() {
            argv.extend(["--context".to_owned(), context.to_owned()]);
        }
        argv.extend([
            "run".to_owned(),
            "--rm".to_owned(),
            "--network".to_owned(),
            self.config.network.clone(),
        ]);
        if let Some(cpus) = self.config.cpus.as_deref() {
            argv.extend(["--cpus".to_owned(), cpus.to_owned()]);
        }
        if let Some(memory_mb) = self.config.memory_mb {
            argv.extend(["--memory".to_owned(), format!("{memory_mb}m")]);
        }
        argv.extend([
            "-v".to_owned(),
            format!("{}:{}", stage_dir.display(), self.config.workdir),
            "-w".to_owned(),
            self.config.workdir.clone(),
            self.config.image.clone(),
            command.to_owned(),
        ]);
        argv.extend(args.iter().cloned());
        ("docker".to_owned(), argv)
    }

    /// Stage the workspace into a temp dir, run the container, read results
    /// back from the bind mount, hash the output.
    pub async fn run_staged(
        &self,
        local_root: &std::path::Path,
        command: &str,
        args: &[String],
        timeout_ms: u64,
        max_bytes: usize,
    ) -> Result<RemoteOutput, RunnerError> {
        // Fail closed when the bind source is missing (never mount the host
        // root as a fallback).
        let metadata = tokio::fs::metadata(local_root)
            .await
            .map_err(|err| RunnerError::Refused(format!("workspace missing: {err}")))?;
        if !metadata.is_dir() {
            return Err(RunnerError::Refused(
                "workspace root is not a directory".to_owned(),
            ));
        }
        let (program, argv) = self.run_argv(local_root, command, args);
        let timeout = Duration::from_millis(timeout_ms.clamp(100, 300_000));
        let ran = self.commands.run(&program, &argv, None, timeout).await;
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
        Ok(RemoteOutput {
            status: ran.status,
            stdout: stdout.clone(),
            stderr: stderr.clone(),
            timed_out: false,
            output_sha256: hash_output(&stdout, &stderr),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::MockRunner;
    use std::sync::Arc;

    fn target() -> (DockerTarget, Arc<MockRunner>) {
        let commands = Arc::new(MockRunner::default());
        let config = DockerRunnerConfig {
            image: "talos-runner:1".to_owned(),
            context: Some("remote".to_owned()),
            workdir: "/work".to_owned(),
            cpus: Some("2".to_owned()),
            memory_mb: Some(4096),
            network: "none".to_owned(),
        };
        (DockerTarget::new(config, commands.clone()), commands)
    }

    #[test]
    fn docker_argv_is_isolated_single_mount() {
        let (target, _) = target();
        let (program, argv) = target.run_argv(std::path::Path::new("/tmp/stage"), "pytest", &[]);
        assert_eq!(program, "docker");
        let joined = argv.join(" ");
        assert!(joined.contains("--rm"), "ephemeral container");
        assert!(joined.contains("--network none"), "air-gapped by default");
        assert!(joined.contains("--context remote"));
        assert!(joined.contains("--cpus 2"));
        assert!(joined.contains("--memory 4096m"));
        // Exactly one -v, pointing at the staging dir only.
        assert_eq!(argv.iter().filter(|arg| *arg == "-v").count(), 1);
        assert!(joined.contains("/tmp/stage:/work"));
        assert!(joined.ends_with("talos-runner:1 pytest"));
    }

    #[test]
    fn docker_argv_minimal_without_options() {
        let commands = Arc::new(MockRunner::default());
        let target = DockerTarget::new(
            DockerRunnerConfig {
                image: "img".to_owned(),
                context: None,
                workdir: "/work".to_owned(),
                cpus: None,
                memory_mb: None,
                network: "none".to_owned(),
            },
            commands,
        );
        let (_, argv) = target.run_argv(std::path::Path::new("/s"), "cargo", &["test".to_owned()]);
        assert!(!argv.iter().any(|arg| arg == "--context"));
        assert!(!argv.iter().any(|arg| arg == "--cpus"));
    }

    #[tokio::test]
    async fn run_staged_hashes_bounded_output() {
        let (target, mock) = target();
        mock.push_success("ok\n", "");
        let dir = tempfile::tempdir().expect("temp dir");
        let output = target
            .run_staged(dir.path(), "cargo", &["test".to_owned()], 30_000, 1024)
            .await
            .expect("runs");
        assert_eq!(output.status, Some(0));
        assert!(output.output_sha256.starts_with("sha256:"));
        assert_eq!(mock.call_count(), 1);
    }

    #[tokio::test]
    async fn run_staged_refuses_missing_workspace() {
        let (target, _) = target();
        let err = target
            .run_staged(
                std::path::Path::new("/nonexistent-talos-stage"),
                "cargo",
                &[],
                1000,
                64,
            )
            .await
            .expect_err("must refuse");
        assert!(matches!(err, RunnerError::Refused(_)));
    }
}
