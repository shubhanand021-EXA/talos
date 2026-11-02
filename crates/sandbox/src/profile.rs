//! Sandbox profiles, enforcement configuration, and receipts.
//!
//! Wire shapes mirror `@talos/contracts` `sandbox.ts` (`SandboxConfig`,
//! `SandboxReceipt`): field names are camelCase on the wire so the
//! TypeScript schemas and these structs can never drift.

use std::path::PathBuf;

use crate::err;

/// OS sandbox profile for shell/process execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxProfile {
    /// Read-only: no process execution (reads only).
    ReadOnly,
    /// Read + write inside the workspace, OS-sandboxed, no network by default.
    #[default]
    WorkspaceWrite,
    /// Full user access (no OS confinement; explicit human approval required).
    DangerFullAccess,
}

impl SandboxProfile {
    /// Stable wire value (`read_only` | `workspace_write` | `danger_full_access`).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read_only",
            Self::WorkspaceWrite => "workspace_write",
            Self::DangerFullAccess => "danger_full_access",
        }
    }

    /// Parse a wire value (case-insensitive, `-`/` ` accepted as `_`).
    pub fn parse(value: &str) -> crate::Result<Self> {
        match value
            .trim()
            .to_ascii_lowercase()
            .replace(['-', ' '], "_")
            .as_str()
        {
            "read_only" | "readonly" => Ok(Self::ReadOnly),
            "workspace_write" | "workspacewrite" => Ok(Self::WorkspaceWrite),
            "danger_full_access" | "dangerfullaccess" | "danger" => Ok(Self::DangerFullAccess),
            other => Err(err(&format!("unknown sandbox profile: {other}"))),
        }
    }
}

/// OS sandbox enforcement configuration (mirrors the TS `SandboxConfig`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxConfig {
    /// Enforcement profile.
    pub profile: SandboxProfile,
    /// Workspace root (bind target; canonicalized before mounting).
    pub workspace_root: PathBuf,
    /// Outbound network allowed (default false).
    pub network_allowed: bool,
    /// Explicit egress allowlist (`host` or `host:port`; empty = none).
    pub allowed_hosts: Vec<String>,
    /// CPU cap percent (1..=100; best-effort — see platform docs).
    pub cpu_limit_percent: Option<u8>,
    /// Memory cap in MiB (16..=65536).
    pub memory_limit_mb: Option<u32>,
    /// Hard timeout in milliseconds (100..=300_000).
    pub timeout_ms: u64,
    /// Exact executable names the sandbox may launch.
    pub allowed_commands: Vec<String>,
    /// Granted danger approval id (required when `profile` is danger).
    /// `None` for every other profile. Never a secret — an approval id only.
    pub danger_approval_id: Option<String>,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            profile: SandboxProfile::WorkspaceWrite,
            workspace_root: std::env::temp_dir(),
            network_allowed: false,
            allowed_hosts: Vec::new(),
            cpu_limit_percent: None,
            memory_limit_mb: None,
            timeout_ms: 60_000,
            allowed_commands: Vec::new(),
            danger_approval_id: None,
        }
    }
}

impl SandboxConfig {
    /// Validate ranges (hosts, limits, timeout, command list).
    pub fn validate(&self) -> crate::Result<()> {
        if self.workspace_root.as_os_str().is_empty() {
            return Err(err("workspace_root must not be empty"));
        }
        if self.allowed_hosts.len() > 16 {
            return Err(err("allowed_hosts exceeds 16 entries"));
        }
        for host in &self.allowed_hosts {
            if host.trim().is_empty() || host.len() > 253 {
                return Err(err("allowed_hosts entries must be 1..=253 chars"));
            }
        }
        if let Some(cpu) = self.cpu_limit_percent {
            if !(1..=100).contains(&cpu) {
                return Err(err("cpu_limit_percent must be 1..=100"));
            }
        }
        if let Some(memory) = self.memory_limit_mb {
            if !(16..=65_536).contains(&memory) {
                return Err(err("memory_limit_mb must be 16..=65536"));
            }
        }
        if !(100..=300_000).contains(&self.timeout_ms) {
            return Err(err("timeout_ms must be 100..=300000"));
        }
        if self.allowed_commands.len() > 64 {
            return Err(err("allowed_commands exceeds 64 entries"));
        }
        if self.profile == SandboxProfile::DangerFullAccess
            && self
                .danger_approval_id
                .as_deref()
                .is_none_or(|id| id.trim().is_empty())
        {
            return Err(err(
                "danger_full_access requires a granted danger_approval_id",
            ));
        }
        Ok(())
    }
}

/// Human-readable egress policy actually applied.
#[must_use]
pub fn network_policy(config: &SandboxConfig) -> String {
    if config.network_allowed {
        if config.allowed_hosts.is_empty() {
            "host-network (egress allowlisted at policy layer only)".to_owned()
        } else {
            format!(
                "host-network (policy allowlist: {})",
                config.allowed_hosts.join(", ")
            )
        }
    } else {
        "no-network (OS blocked)".to_owned()
    }
}
///
/// Recorded in tool output JSON and persisted as evidence with the receipt.
/// Sandbox enforcement receipt (mirrors the TS `SandboxReceipt`).
///
/// Recorded in tool output JSON and persisted as evidence with the receipt.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxReceipt {
    /// Profile that was enforced (or requested, when denied).
    pub profile: SandboxProfile,
    /// True only when an OS backend actually confined the child.
    pub enforced: bool,
    /// Backend description (e.g. `bubblewrap+prlimit`, `deny-exec(read-only)`).
    pub enforcement_method: String,
    /// Egress policy actually applied.
    pub network_policy: String,
    /// Runtime violations observed (resource kills, escape attempts).
    pub violations: Vec<String>,
}

impl SandboxReceipt {
    /// Denied-before-spawn receipt (fail closed or profile denial).
    #[must_use]
    pub fn denied(profile: SandboxProfile, method: &str, reason: &str) -> Self {
        Self {
            profile,
            enforced: false,
            enforcement_method: method.to_owned(),
            network_policy: "n/a (execution denied)".to_owned(),
            violations: vec![reason.to_owned()],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_parse_and_round_trip() {
        assert_eq!(
            SandboxProfile::parse("read_only").unwrap(),
            SandboxProfile::ReadOnly
        );
        assert_eq!(
            SandboxProfile::parse("workspace_write").unwrap(),
            SandboxProfile::WorkspaceWrite
        );
        assert_eq!(
            SandboxProfile::parse("danger_full_access").unwrap(),
            SandboxProfile::DangerFullAccess
        );
        assert_eq!(
            SandboxProfile::parse("DANGER").unwrap(),
            SandboxProfile::DangerFullAccess
        );
        assert!(SandboxProfile::parse("unconfined").is_err());
        assert_eq!(SandboxProfile::default(), SandboxProfile::WorkspaceWrite);
        // Wire values match the TS enum exactly.
        assert_eq!(SandboxProfile::ReadOnly.as_str(), "read_only");
        assert_eq!(SandboxProfile::WorkspaceWrite.as_str(), "workspace_write");
        assert_eq!(
            SandboxProfile::DangerFullAccess.as_str(),
            "danger_full_access"
        );
    }

    #[test]
    fn config_defaults_to_closed_network() {
        let config = SandboxConfig::default();
        assert!(!config.network_allowed);
        assert!(config.allowed_hosts.is_empty());
        assert_eq!(config.profile, SandboxProfile::WorkspaceWrite);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn config_validates_ranges() {
        let mut config = SandboxConfig {
            cpu_limit_percent: Some(0),
            ..SandboxConfig::default()
        };
        assert!(config.validate().is_err());
        config.cpu_limit_percent = Some(50);
        assert!(config.validate().is_ok());
        config.memory_limit_mb = Some(8);
        assert!(config.validate().is_err());
        config.memory_limit_mb = Some(512);
        assert!(config.validate().is_ok());
        config.timeout_ms = 50;
        assert!(config.validate().is_err());
        config.timeout_ms = 60_000;
        assert!(config.validate().is_ok());
        config.allowed_hosts = vec!["x".to_owned(); 17];
        assert!(config.validate().is_err());
        config.allowed_hosts.clear();
        // Danger without an approval id is rejected at validation.
        config.profile = SandboxProfile::DangerFullAccess;
        assert!(config.validate().is_err());
        config.danger_approval_id = Some("appr-task-1".to_owned());
        assert!(config.validate().is_ok());
    }
}
