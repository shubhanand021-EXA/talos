//! Runner registry file: `.talos/runners.json` (user's BYOC fleet).
//!
//! Secret-free: SSH private keys never cross this file — only the key *path*
//! (0600 user file, read by the local `ssh` client at spawn). Passphrases are
//! unsupported by design (use unencrypted deploy keys scoped to one runner).

use std::path::Path;

use talos_common::{Result, TalosError};

/// Config file name inside the app-data dir.
pub const RUNNERS_FILE_NAME: &str = "runners.json";

/// One BYOC runner.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RunnerConfig {
    /// Stable id (`[a-z0-9-_]{1,64}`), referenced by tasks.
    pub id: String,
    /// Transport + target.
    pub kind: RunnerKind,
    /// Whether tasks may target this runner.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

/// SSH (`ssh` + `tar` pipe, both ubiquitous) or Docker (`docker run`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "transport", rename_all = "lowercase")]
pub enum RunnerKind {
    /// OpenSSH target.
    Ssh(SshRunnerConfig),
    /// Docker daemon target (local daemon or `--context`).
    Docker(DockerRunnerConfig),
}

/// OpenSSH target: unprivileged user, one isolated remote directory.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SshRunnerConfig {
    /// Hostname or IP.
    pub host: String,
    /// Remote user (unprivileged, e.g. `talos`).
    pub user: String,
    /// Private key path (`~` expands to the home dir).
    pub key_path: String,
    /// SSH port.
    #[serde(default = "default_ssh_port")]
    pub port: u16,
    /// Isolated remote working dir (created on first use, nothing else touched).
    pub remote_dir: String,
    /// `StrictHostKeyChecking` value (`accept-new` default, `yes` for TOFU-free).
    #[serde(default = "default_strict_host_key_checking")]
    pub strict_host_key_checking: String,
    /// Connect timeout in seconds.
    #[serde(default = "default_connect_timeout")]
    pub connect_timeout_secs: u64,
}

fn default_ssh_port() -> u16 {
    22
}

fn default_strict_host_key_checking() -> String {
    "accept-new".to_owned()
}

fn default_connect_timeout() -> u64 {
    10
}

/// Docker target: fresh `--rm` container per invocation, exactly one
/// bind-mount (the staged workspace), no network.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DockerRunnerConfig {
    /// Image to run (required — fail closed without one).
    pub image: String,
    /// Optional `docker --context` (shares the daemon's filesystem view).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    /// Container workdir (bind-mounted staging dir).
    #[serde(default = "default_workdir")]
    pub workdir: String,
    /// CPU limit (`--cpus`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpus: Option<String>,
    /// Memory limit in MB (`--memory`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_mb: Option<u64>,
    /// Network mode (`none` default — air-gapped unless explicitly opened).
    #[serde(default = "default_network")]
    pub network: String,
}

fn default_workdir() -> String {
    "/work".to_owned()
}

fn default_network() -> String {
    "none".to_owned()
}

/// Whole-file registry.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct RunnerRegistryFile {
    /// Runners (ids must be unique).
    #[serde(default)]
    pub runners: Vec<RunnerConfig>,
}

impl RunnerRegistryFile {
    /// Load from `dir/runners.json` (missing ⇒ empty; corrupt ⇒ error).
    pub fn load(dir: &Path) -> Result<Self> {
        let path = dir.join(RUNNERS_FILE_NAME);
        let text = std::fs::read_to_string(&path)
            .map_err(|err| TalosError::Storage(format!("read {}: {err}", path.display())))?;
        let file: Self = serde_json::from_str(&text)
            .map_err(|err| TalosError::Storage(format!("parse {}: {err}", path.display())))?;
        file.validate()?;
        Ok(file)
    }

    /// Load or default-empty when the file is absent (first-run friendly).
    #[must_use]
    pub fn load_or_default(dir: &Path) -> Self {
        if !dir.join(RUNNERS_FILE_NAME).exists() {
            return Self::default();
        }
        Self::load(dir).unwrap_or_default()
    }

    /// Find one enabled runner by id.
    #[must_use]
    pub fn find_enabled(&self, id: &str) -> Option<&RunnerConfig> {
        self.runners
            .iter()
            .find(|runner| runner.id == id && runner.enabled)
    }

    /// Validate ids unique, fields well-formed.
    pub fn validate(&self) -> Result<()> {
        let mut seen = std::collections::HashSet::new();
        for runner in &self.runners {
            if runner.id.trim().is_empty()
                || runner.id.len() > 64
                || !runner
                    .id
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
            {
                return Err(TalosError::Storage(format!(
                    "runner id must be 1..=64 chars [a-z0-9-_]: '{}'",
                    runner.id
                )));
            }
            if !seen.insert(runner.id.clone()) {
                return Err(TalosError::Storage(format!(
                    "duplicate runner id: '{}'",
                    runner.id
                )));
            }
            match &runner.kind {
                RunnerKind::Ssh(ssh) => {
                    for (what, value) in [
                        ("host", &ssh.host),
                        ("user", &ssh.user),
                        ("keyPath", &ssh.key_path),
                        ("remoteDir", &ssh.remote_dir),
                    ] {
                        if value.trim().is_empty() || value.len() > 1024 {
                            return Err(TalosError::Storage(format!(
                                "ssh runner '{}': {what} must be 1..=1024 chars",
                                runner.id
                            )));
                        }
                    }
                    if ssh.port == 0 {
                        return Err(TalosError::Storage(format!(
                            "ssh runner '{}': invalid port",
                            runner.id
                        )));
                    }
                }
                RunnerKind::Docker(docker) => {
                    if docker.image.trim().is_empty() || docker.image.len() > 512 {
                        return Err(TalosError::Storage(format!(
                            "docker runner '{}': image must be 1..=512 chars",
                            runner.id
                        )));
                    }
                }
            }
        }
        Ok(())
    }
}

/// Expand a leading `~/` against `HOME` / `USERPROFILE`.
#[must_use]
pub fn expand_home(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")) {
        if let Ok(home) = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
            return format!("{home}/{rest}");
        }
    }
    path.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ssh_runner(id: &str) -> RunnerConfig {
        RunnerConfig {
            id: id.to_owned(),
            kind: RunnerKind::Ssh(SshRunnerConfig {
                host: "gpu.local".to_owned(),
                user: "talos".to_owned(),
                key_path: "~/.ssh/talos_ed25519".to_owned(),
                port: 22,
                remote_dir: "~/.talos-runs".to_owned(),
                strict_host_key_checking: "accept-new".to_owned(),
                connect_timeout_secs: 10,
            }),
            enabled: true,
        }
    }

    #[test]
    fn validates_ids_and_dupes() {
        let mut file = RunnerRegistryFile {
            runners: vec![ssh_runner("gpu-box"), ssh_runner("gpu-box")],
        };
        assert!(file.validate().is_err());
        file.runners.pop();
        assert!(file.validate().is_ok());
        file.runners[0].id = "Bad ID!".to_owned();
        assert!(file.validate().is_err());
    }

    #[test]
    fn docker_requires_image() {
        let file = RunnerRegistryFile {
            runners: vec![RunnerConfig {
                id: "d".to_owned(),
                kind: RunnerKind::Docker(DockerRunnerConfig {
                    image: String::new(),
                    context: None,
                    workdir: "/work".to_owned(),
                    cpus: None,
                    memory_mb: None,
                    network: "none".to_owned(),
                }),
                enabled: true,
            }],
        };
        assert!(file.validate().is_err());
    }

    #[test]
    fn missing_file_loads_empty() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = RunnerRegistryFile::load_or_default(dir.path());
        assert!(file.runners.is_empty());
        assert!(file.find_enabled("x").is_none());
    }

    #[test]
    fn expands_home_prefix() {
        assert!(expand_home("~/k").ends_with("/k") || expand_home("~/k") == "~/k");
        assert_eq!(expand_home("/abs/k"), "/abs/k");
    }
}
