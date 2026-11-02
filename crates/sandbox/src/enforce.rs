//! OS sandbox enforcement: trait, platform dispatch, fail-closed gating.
//!
//! [`SandboxEnforcer`] backends wrap a [`tokio::process::Command`] so the
//! child starts confined (namespaces on Linux, Seatbelt on macOS, Job
//! Objects on Windows) and run a post-spawn hook ([`PostSpawnGuard`]) for
//! limits that need a live pid (cgroup placement, job assignment).
//!
//! [`enforce_or_deny`] is the single fail-closed entry point: read-only
//! denies execution outright, danger requires a granted approval id, and
//! confined profiles deny with `sandbox_unavailable` when no backend can
//! be established — never falling back to unsandboxed execution.

use crate::profile::{SandboxConfig, SandboxProfile};
use crate::{err, Result};

/// Stable error code for fail-closed denials (surfaced in tool receipts).
pub const SANDBOX_UNAVAILABLE: &str = "sandbox_unavailable";

/// Test hook: when set (any value), every backend reports unavailable, so
/// the fail-closed path is deterministic on any platform.
pub const FORCE_UNAVAILABLE_ENV: &str = "TALOS_SANDBOX_FORCE_UNAVAILABLE";

/// Shell command pieces extracted from a caller-built command.
///
/// The sandbox backend rebuilds the full child from these parts (wrapping
/// the program with `bwrap`/`sandbox-exec`, re-applying env/cwd, and
/// imposing the standard spawn shape: piped stdout/stderr, null stdin,
/// `kill_on_drop`, and a fresh Unix process group for tree-kill).
#[derive(Debug, Clone)]
pub struct CommandSpec {
    /// Executable name (allowlisted upstream).
    pub program: std::ffi::OsString,
    /// Arguments.
    pub args: Vec<std::ffi::OsString>,
    /// Explicit environment (`None` = removed).
    pub envs: Vec<(std::ffi::OsString, Option<std::ffi::OsString>)>,
    /// Working directory.
    pub cwd: std::path::PathBuf,
}

impl CommandSpec {
    /// Extract the spawn pieces from a caller-built command.
    #[must_use]
    pub fn from_command(cmd: &tokio::process::Command) -> Self {
        let std_cmd = cmd.as_std();
        Self {
            program: std_cmd.get_program().to_os_string(),
            args: std_cmd.get_args().map(|s| s.to_os_string()).collect(),
            envs: std_cmd
                .get_envs()
                .map(|(k, v)| (k.to_os_string(), v.map(|v| v.to_os_string())))
                .collect(),
            cwd: std_cmd
                .get_current_dir()
                .map(std::path::Path::to_path_buf)
                .unwrap_or_else(|| std::env::temp_dir()),
        }
    }
}
///
/// Held until the child is reaped so limits cannot outlive their guard.
/// Post-spawn enforcement handle (cgroup placement, Job Object assignment).
///
/// Held until the child is reaped so limits cannot outlive their guard.
#[derive(Debug, Default)]
pub struct PostSpawnGuard {
    /// Human-readable placement (cgroup path, job id) for receipts.
    pub placement: Option<String>,
    /// Windows Job handle value (closed on drop; Windows only).
    #[cfg(windows)]
    pub handle: Option<isize>,
}

impl Drop for PostSpawnGuard {
    fn drop(&mut self) {
        #[cfg(windows)]
        if let Some(raw) = self.handle.take() {
            crate::windows::close_job(raw);
        }
    }
}

/// Backend implemented by every platform sandbox.
pub trait SandboxEnforcer: Send + Sync + std::fmt::Debug {
    /// Stable backend name (`bubblewrap`, `seatbelt`, `windows-job-objects`).
    fn name(&self) -> &'static str;
    /// Human description of what is enforced for this config.
    fn enforcement_method(&self, config: &SandboxConfig) -> String;
    /// True when the backend can actually confine a child on this machine.
    fn is_available(&self) -> bool;
    /// Why available/unavailable (binary versions, missing tools).
    fn availability_detail(&self) -> String;
    /// Wrap `spec` into a fully confined child command (standard spawn
    /// shape: piped stdout/stderr, null stdin, `kill_on_drop`, fresh Unix
    /// process group for tree-kill where applicable).
    fn wrap_command(
        &self,
        config: &SandboxConfig,
        spec: CommandSpec,
    ) -> Result<tokio::process::Command>;
    /// Post-spawn hook (cgroup placement, job assignment). Default: no-op.
    fn on_spawn(
        &self,
        _config: &SandboxConfig,
        _child: &tokio::process::Child,
    ) -> Result<PostSpawnGuard> {
        Ok(PostSpawnGuard::default())
    }
}

/// Build the standard confined child: argv, env, cwd, piped stdout/stderr,
/// null stdin, `kill_on_drop`, and — on Unix — a fresh process group so the
/// timeout path can tree-kill the sandbox.
#[must_use]
pub fn standard_child(
    program: std::ffi::OsString,
    args: &[std::ffi::OsString],
    envs: &[(std::ffi::OsString, Option<std::ffi::OsString>)],
    cwd: &std::path::Path,
) -> tokio::process::Command {
    let mut std_cmd = std::process::Command::new(program);
    std_cmd.args(args);
    std_cmd.current_dir(cwd);
    std_cmd.stdin(std::process::Stdio::null());
    std_cmd.stdout(std::process::Stdio::piped());
    std_cmd.stderr(std::process::Stdio::piped());
    std_cmd.env_clear();
    for (key, value) in envs {
        if let Some(value) = value {
            std_cmd.env(key, value);
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        // Fresh process group: the child's pgid == its pid, so
        // `kill -KILL -<pid>` takes the entire sandbox tree.
        std_cmd.process_group(0);
    }
    let mut cmd = tokio::process::Command::from(std_cmd);
    // Backstop: dropping the handle without reaping still kills the child.
    cmd.kill_on_drop(true);
    cmd
}

/// One platform backend status (mirrors the TS `SandboxBackend`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxBackendStatus {
    /// Backend name.
    pub name: String,
    /// Whether it can confine a child on this machine.
    pub available: bool,
    /// Enforcement description (or the missing piece).
    pub enforcement_method: String,
    /// Availability detail (versions, limitations).
    pub detail: String,
}

/// Platform sandbox status (mirrors the TS `SandboxStatus`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxStatus {
    /// OS platform (`linux`, `macos`, `windows`, …).
    pub platform: String,
    /// Always true: Talos denies rather than running unsandboxed.
    pub fail_closed: bool,
    /// Per-backend status.
    pub backends: Vec<SandboxBackendStatus>,
    /// RFC 3339 check time.
    pub checked_at: String,
}

/// Force-unavailable test hook state.
fn force_unavailable() -> bool {
    std::env::var_os(FORCE_UNAVAILABLE_ENV).is_some()
}

/// Platform backend for this machine (no availability filtering).
pub fn select_enforcer() -> Box<dyn SandboxEnforcer> {
    #[cfg(target_os = "linux")]
    return Box::new(crate::linux::BubblewrapSandbox::new());
    #[cfg(target_os = "macos")]
    return Box::new(crate::macos::SeatbeltSandbox::new());
    #[cfg(windows)]
    return Box::new(crate::windows::WindowsSandbox::new());
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    return Box::new(UnavailableSandbox);
}

/// Status of every backend known on this platform.
pub fn sandbox_backends() -> Vec<SandboxBackendStatus> {
    let enforcer = select_enforcer();
    let available = enforcer.is_available() && !force_unavailable();
    vec![SandboxBackendStatus {
        name: enforcer.name().to_owned(),
        available,
        enforcement_method: if available {
            format!("{} (confined profiles)", enforcer.name())
        } else {
            "unavailable — execution denied (fail closed)".to_owned()
        },
        detail: if force_unavailable() {
            format!(
                "{} forced unavailable by {FORCE_UNAVAILABLE_ENV} (test hook)",
                enforcer.name()
            )
        } else {
            enforcer.availability_detail()
        },
    }]
}

/// Platform status snapshot for the UI (`get_sandbox_status`).
#[must_use]
pub fn sandbox_status() -> SandboxStatus {
    SandboxStatus {
        platform: std::env::consts::OS.to_owned(),
        fail_closed: true,
        backends: sandbox_backends(),
        checked_at: chrono::Utc::now().to_rfc3339(),
    }
}

/// Fail-closed enforcement gate.
///
/// - `ReadOnly` → `Err` (no process execution, no backend needed).
/// - `DangerFullAccess` → `Err` unless `danger_approval_id` names a granted
///   approval (checked upstream by the desktop layer; absence here denies).
/// - Confined profiles → the platform backend, or `Err(sandbox_unavailable)`
///   when it cannot be established. Never falls back to unsandboxed runs.
pub fn enforce_or_deny(config: &SandboxConfig) -> Result<Box<dyn SandboxEnforcer>> {
    config.validate()?;
    match config.profile {
        SandboxProfile::ReadOnly => Err(err(
            "read_only profile forbids process execution (sandbox_unavailable)",
        )),
        SandboxProfile::DangerFullAccess => {
            if config
                .danger_approval_id
                .as_deref()
                .is_none_or(|id| id.trim().is_empty())
            {
                return Err(err(
                    "danger_full_access requires a granted human approval (sandbox_unavailable)",
                ));
            }
            Err(err(
                "danger_full_access runs with full user access by explicit approval; no OS sandbox applies",
            ))
        }
        SandboxProfile::WorkspaceWrite => {
            if force_unavailable() {
                return Err(err(
                    "OS sandbox backend forced unavailable (sandbox_unavailable)",
                ));
            }
            let enforcer = select_enforcer();
            if !enforcer.is_available() {
                return Err(err(&format!(
                    "{} unavailable ({}); execution denied (sandbox_unavailable)",
                    enforcer.name(),
                    enforcer.availability_detail()
                )));
            }
            Ok(enforcer)
        }
    }
}

/// Fallback backend for unknown platforms: always unavailable (fail closed).
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
#[derive(Debug, Default)]
struct UnavailableSandbox;

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
impl SandboxEnforcer for UnavailableSandbox {
    fn name(&self) -> &'static str {
        "unavailable"
    }
    fn enforcement_method(&self, _config: &SandboxConfig) -> String {
        "unavailable — execution denied (fail closed)".to_owned()
    }
    fn is_available(&self) -> bool {
        false
    }
    fn availability_detail(&self) -> String {
        format!("no OS sandbox backend for {}", std::env::consts::OS)
    }
    fn wrap_command(
        &self,
        _config: &SandboxConfig,
        _cmd: tokio::process::Command,
    ) -> Result<tokio::process::Command> {
        Err(err(
            "no OS sandbox backend on this platform (sandbox_unavailable)",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace_config() -> SandboxConfig {
        SandboxConfig {
            workspace_root: std::env::temp_dir(),
            ..SandboxConfig::default()
        }
    }

    #[test]
    fn read_only_denies_without_backend() {
        let config = SandboxConfig {
            profile: SandboxProfile::ReadOnly,
            ..workspace_config()
        };
        let denied = enforce_or_deny(&config).unwrap_err();
        assert!(denied.to_string().contains("sandbox_unavailable"));
    }

    #[test]
    fn danger_without_approval_denies() {
        let config = SandboxConfig {
            profile: SandboxProfile::DangerFullAccess,
            ..workspace_config()
        };
        // Validation rejects danger without an approval id before any
        // spawn is attempted (fail closed at the gate).
        let denied = enforce_or_deny(&config).unwrap_err();
        assert!(denied.to_string().contains("danger"));
    }

    #[test]
    fn forced_unavailable_fails_closed() {
        std::env::set_var(FORCE_UNAVAILABLE_ENV, "1");
        let denied = enforce_or_deny(&workspace_config()).unwrap_err();
        std::env::remove_var(FORCE_UNAVAILABLE_ENV);
        assert!(denied.to_string().contains("sandbox_unavailable"));
        // Backend status reports the forced outage honestly.
        std::env::set_var(FORCE_UNAVAILABLE_ENV, "1");
        let status = sandbox_status();
        std::env::remove_var(FORCE_UNAVAILABLE_ENV);
        assert!(status.fail_closed);
        assert!(!status.backends.iter().any(|b| b.available));
    }

    #[test]
    fn status_snapshot_is_shaped() {
        let status = sandbox_status();
        assert_eq!(status.platform, std::env::consts::OS);
        assert!(status.fail_closed);
        assert_eq!(status.backends.len(), 1);
        assert!(!status.checked_at.is_empty());
    }
}
