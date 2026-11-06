//! BYOC remote execution targets for Talos tools.
//!
//! Policy evaluation always happens locally *before* any remote command runs
//! (see `tool_runtime::executor`): this crate only moves bytes after the
//! broker Allows. SSH runners sync the workspace over a `tar` pipe (no
//! `rsync` dependency, no local shell) and run commands via `ssh` under an
//! unprivileged remote user inside one isolated directory. Docker runners
//! bind-mount exactly the staged workspace (`-v stage:/work`, `--network
//! none`, fresh container per invocation) — never the host root.

pub mod config;
pub mod docker;
pub mod runner;
pub mod ssh;

pub use config::{
    DockerRunnerConfig, RunnerConfig, RunnerKind, SshRunnerConfig, RUNNERS_FILE_NAME,
};
pub use docker::DockerTarget;
pub use runner::{
    bound_text, hash_output, CommandOutput, CommandRunner, MockRunner, RemoteOutput, RunnerError,
    SystemRunner,
};
pub use ssh::SshTarget;

/// A resolved remote runner: validated config plus its command transport.
/// Cheap to clone (`Arc` command runner); `Debug` never prints secrets.
#[derive(Clone)]
pub struct RemoteRunner {
    config: RunnerConfig,
    commands: std::sync::Arc<dyn CommandRunner>,
}

impl std::fmt::Debug for RemoteRunner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteRunner")
            .field("id", &self.config.id)
            .field("kind", &self.config.kind)
            .finish()
    }
}

impl RemoteRunner {
    /// Resolve one enabled runner by id (fails closed on unknown/disabled).
    pub fn new(config: RunnerConfig, commands: std::sync::Arc<dyn CommandRunner>) -> Self {
        Self { config, commands }
    }

    /// Runner id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.config.id
    }

    /// Runner kind.
    #[must_use]
    pub fn kind(&self) -> &RunnerKind {
        &self.config.kind
    }

    /// Sync the local workspace root to the target and run `command args`
    /// there with `timeout_ms`, then copy results back (SSH) or read them
    /// from the bind-mounted staging dir (Docker). Returns the bounded
    /// output plus its `sha256:` hash for the evidence ledger.
    pub async fn execute(
        &self,
        local_root: &std::path::Path,
        command: &str,
        args: &[String],
        timeout_ms: u64,
        max_bytes: usize,
    ) -> Result<runner::RemoteOutput, RunnerError> {
        match &self.config.kind {
            RunnerKind::Ssh(ssh) => {
                let target = SshTarget::new(ssh.clone(), self.commands.clone());
                target
                    .run_synced(local_root, command, args, timeout_ms, max_bytes)
                    .await
            }
            RunnerKind::Docker(docker) => {
                let target = DockerTarget::new(docker.clone(), self.commands.clone());
                target
                    .run_staged(local_root, command, args, timeout_ms, max_bytes)
                    .await
            }
        }
    }
}
