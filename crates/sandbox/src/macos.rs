//! macOS backend: Seatbelt (`sandbox-exec`) profiles + availability probe.
//!
//! The `.sb` profile is generated purely from the [`SandboxConfig`] (fully
//! unit-testable on any platform). Enforcement runs only on macOS via
//! `sandbox-exec -f <profile> <program> <args>`; elsewhere the backend
//! reports unavailable and execution fails closed.
//!
//! Resource limits: Seatbelt has no rlimit equivalent — CPU/memory/process
//! bounds come from the existing timeout kill machinery (documented
//! limitation; exact percent caps are Linux-cgroup/Windows-Job territory).

use crate::profile::SandboxConfig;
use crate::{err, Result};

/// Override for the `sandbox-exec` binary (tests point it at nothing).
pub const SANDBOX_EXEC_ENV: &str = "TALOS_SANDBOX_EXEC_BIN";
/// Default `sandbox-exec` binary.
pub const SANDBOX_EXEC_BIN: &str = "sandbox-exec";

/// macOS Seatbelt backend.
#[derive(Debug, Default)]
pub struct SeatbeltSandbox;

impl SeatbeltSandbox {
    /// Create the backend.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// `sandbox-exec` binary (env override for tests).
    fn sandbox_exec_bin() -> String {
        std::env::var(SANDBOX_EXEC_ENV).unwrap_or_else(|_| SANDBOX_EXEC_BIN.to_owned())
    }

    /// True only on macOS with a runnable `sandbox-exec`.
    fn sandbox_exec_present() -> bool {
        if !cfg!(target_os = "macos") {
            return false;
        }
        std::process::Command::new(Self::sandbox_exec_bin())
            .arg("-h")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }
}

/// Escape a path for embedding in a Seatbelt string literal.
fn seatbelt_escape(path: &str) -> String {
    path.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Generate a Seatbelt `.sb` profile from a config (pure; unit-tested).
///
/// - version 1, default deny.
/// - Read: `file-read*` everywhere (toolchain + workspace reads).
/// - Writes: denied by default; `file-write*` under the workspace when the
///   profile is `workspace_write` (literal + `/*` subtree).
/// - Network: `deny network*` unless `network_allowed`; explicit
///   `allow network-outbound (remote tcp "host:port")` per allowed host.
/// - Process: allow `process-exec` (spawning the confined child is the
///   point); no other Mach/syscall carve-outs.
#[must_use]
pub fn seatbelt_profile(config: &SandboxConfig, workspace: &str) -> String {
    let workspace = seatbelt_escape(workspace);
    let mut profile =
        String::from("(version 1)\n(deny default)\n(allow file-read*)\n(allow process-exec)\n");
    if config.profile == crate::profile::SandboxProfile::WorkspaceWrite {
        profile.push_str(&format!(
            "(allow file-write* (subpath \"{workspace}\"))\n(allow file-write* (literal \"{workspace}\"))\n"
        ));
    } else {
        profile.push_str("(deny file-write*)\n");
    }
    if config.network_allowed {
        for host in &config.allowed_hosts {
            let host = seatbelt_escape(host);
            profile.push_str(&format!(
                "(allow network-outbound (remote tcp \"{host}\"))\n"
            ));
        }
        if config.allowed_hosts.is_empty() {
            profile.push_str("(allow network*)\n");
        }
    } else {
        profile.push_str("(deny network*)\n");
    }
    profile
}

impl crate::enforce::SandboxEnforcer for SeatbeltSandbox {
    fn name(&self) -> &'static str {
        "seatbelt"
    }

    fn enforcement_method(&self, config: &SandboxConfig) -> String {
        let net = if config.network_allowed {
            if config.allowed_hosts.is_empty() {
                "host-network (no host allowlist)"
            } else {
                "host-network (per-host seatbelt rules)"
            }
        } else {
            "no-network (deny network*)"
        };
        format!("seatbelt [{net}; timeout-kill only (no rlimit on macOS)]")
    }

    fn is_available(&self) -> bool {
        Self::sandbox_exec_present()
    }

    fn availability_detail(&self) -> String {
        if !cfg!(target_os = "macos") {
            return format!("seatbelt requires macOS (this is {})", std::env::consts::OS);
        }
        if Self::sandbox_exec_present() {
            "sandbox-exec present".to_owned()
        } else {
            format!(
                "sandbox-exec binary {:?} not runnable",
                Self::sandbox_exec_bin()
            )
        }
    }

    fn wrap_command(
        &self,
        config: &SandboxConfig,
        spec: crate::enforce::CommandSpec,
    ) -> Result<tokio::process::Command> {
        if !self.is_available() {
            return Err(err(&format!(
                "seatbelt unavailable ({}); execution denied (sandbox_unavailable)",
                self.availability_detail()
            )));
        }
        let workspace = config
            .workspace_root
            .canonicalize()
            .map_err(|e| err(&format!("workspace root unavailable: {e}")))?;
        let profile_text = seatbelt_profile(config, &workspace.to_string_lossy());
        let profile_path = workspace.join(".talos-seatbelt.sb");
        std::fs::write(&profile_path, profile_text)
            .map_err(|e| err(&format!("seatbelt profile write: {e}")))?;
        let program: std::ffi::OsString = Self::sandbox_exec_bin().into();
        let mut args: Vec<std::ffi::OsString> =
            vec!["-f".into(), profile_path.into_os_string(), spec.program];
        args.extend(spec.args);
        Ok(crate::enforce::standard_child(
            program,
            &args,
            &spec.envs,
            &workspace,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enforce::SandboxEnforcer;

    fn workspace_config() -> SandboxConfig {
        SandboxConfig {
            workspace_root: std::path::PathBuf::from("/tmp/ws-1"),
            ..SandboxConfig::default()
        }
    }

    #[test]
    fn seatbelt_profile_denies_writes_and_network_by_default() {
        let profile = seatbelt_profile(&workspace_config(), "/tmp/ws-1");
        assert!(profile.starts_with("(version 1)"));
        assert!(profile.contains("(deny default)"));
        assert!(profile.contains("(deny network*)"));
        assert!(profile.contains("(allow file-write* (subpath \"/tmp/ws-1\"))"));
    }

    #[test]
    fn seatbelt_profile_locks_writes_for_read_only() {
        let mut config = workspace_config();
        config.profile = crate::profile::SandboxProfile::ReadOnly;
        let profile = seatbelt_profile(&config, "/tmp/ws-1");
        assert!(profile.contains("(deny file-write*)"));
        assert!(!profile.contains("subpath"));
    }

    #[test]
    fn seatbelt_profile_allows_listed_hosts() {
        let mut config = workspace_config();
        config.network_allowed = true;
        config.allowed_hosts = vec!["proxy.internal:8080".to_owned()];
        let profile = seatbelt_profile(&config, "/tmp/ws-1");
        assert!(!profile.contains("(deny network*)"));
        assert!(profile.contains("(allow network-outbound (remote tcp \"proxy.internal:8080\"))"));
    }

    #[test]
    fn unavailable_off_macos() {
        if cfg!(target_os = "macos") {
            return;
        }
        let backend = SeatbeltSandbox::new();
        assert!(!backend.is_available());
        assert!(backend.availability_detail().contains("requires macOS"));
    }

    #[test]
    fn missing_binary_is_detected() {
        std::env::set_var(SANDBOX_EXEC_ENV, "/nonexistent/sandbox-exec-for-tests");
        assert!(!SeatbeltSandbox::sandbox_exec_present());
        std::env::remove_var(SANDBOX_EXEC_ENV);
    }
}
