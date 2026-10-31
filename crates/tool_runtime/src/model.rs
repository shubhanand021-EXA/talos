//! Tool model: identifiers, manifests, invocations, and receipts.
//!
//! Every tool exposes a [`ToolManifest`] (metadata + risk + schemas for UI
//! and future Tool-RAG), accepts a validated [`ToolInvocation`], and returns
//! a [`ToolReceipt`] that is always persisted as evidence — including denials.

use policy_broker::{Capability, PolicyDecision, RiskLevel};
use talos_common::{Result, TalosError};

/// Canonical tool identifiers (stable wire values).
pub const FS_READ: &str = "fs.read";
/// List a directory (bounded, no symlink escape).
pub const FS_LIST: &str = "fs.list";
/// Search text content (bounded result set).
pub const FS_SEARCH: &str = "fs.search";
/// Write a file (policy + evidence + compensation required).
pub const FS_WRITE: &str = "fs.write";
/// Execute a subprocess (disabled by default, allowlisted, timed out).
pub const PROCESS_EXEC: &str = "process.exec";

/// All built-in tool identifiers in registration order.
pub const BUILT_IN_TOOLS: &[&str] = &[FS_READ, FS_LIST, FS_SEARCH, FS_WRITE, PROCESS_EXEC];

/// Execution mode: dry-run never touches the world; live may (after policy).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionMode {
    /// Plan only: validate + policy-evaluate + describe effects.
    DryRun,
    /// Execute for real (policy must allow first).
    Live,
}

/// Structured tool error (transport for failed/denied invocations).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ToolErrorDetail {
    /// Stable code (`unknown_tool`, `tool_disabled`, `invalid_params`,
    /// `policy_denied`, `sandbox_unavailable`, `io_error`, ...).
    pub code: String,
    /// Human-readable message (safe to display).
    pub message: String,
}

impl ToolErrorDetail {
    /// Build a detail row.
    #[must_use]
    pub fn new(code: &str, message: String) -> Self {
        Self {
            code: code.to_owned(),
            message,
        }
    }
}

/// Terminal status of one invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus {
    /// Completed (or planned, for dry-run).
    Success,
    /// Executed but the tool itself failed.
    Failed,
    /// Policy did not allow execution.
    Denied,
    /// Tool is disabled (live `process.exec` by default).
    Disabled,
}

/// Self-describing tool manifest (registry metadata + UI + Tool-RAG input).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolManifest {
    /// Stable identifier (`fs.read`, ...).
    pub id: String,
    /// Display name.
    pub name: String,
    /// Manifest version.
    pub version: String,
    /// What the tool does.
    pub description: String,
    /// Policy capabilities exercised.
    pub capabilities: Vec<Capability>,
    /// Blast radius.
    pub risk_level: RiskLevel,
    /// Whether effects are undoable.
    pub reversible: bool,
    /// Whether live execution needs recorded evidence first.
    pub requires_evidence: bool,
    /// Whether live execution needs a compensation plan first.
    pub requires_compensation: bool,
    /// False for `process.exec` (explicit opt-in required).
    pub enabled_by_default: bool,
    /// JSON-Schema-style input shape (UI forms + RAG).
    pub input_schema: serde_json::Value,
    /// JSON-Schema-style output shape.
    pub output_schema: serde_json::Value,
}

/// One validated tool call.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolInvocation {
    /// Tool identifier.
    pub tool_id: String,
    /// Owning task (ledger linkage).
    pub task_id: String,
    /// Requesting agent.
    pub agent_id: String,
    /// Tool parameters (validated against the manifest).
    pub params: serde_json::Value,
    /// True for plan-only runs.
    pub dry_run: bool,
}

/// Auditable outcome of one invocation (always stored as evidence).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolReceipt {
    /// Receipt identifier.
    pub id: String,
    /// Tool that ran.
    pub tool_id: String,
    /// Owning task.
    pub task_id: String,
    /// True when nothing was mutated.
    pub dry_run: bool,
    /// Outcome.
    pub status: ToolStatus,
    /// Tool output (planned effects for dry-run).
    pub output: serde_json::Value,
    /// Structured failure, if any.
    pub error: Option<ToolErrorDetail>,
    /// Policy verdict that authorized (or blocked) execution.
    pub policy_decision: PolicyDecision,
    /// Policy rationale.
    pub policy_reason: String,
    /// Evidence row holding this receipt, if persisted.
    pub evidence_id: Option<String>,
    /// Compensation plan covering this invocation, if any.
    pub compensation_plan_id: Option<String>,
    /// RFC 3339 execution timestamp.
    pub executed_at: String,
}

/// Map a tool to its policy capability (primary).
#[must_use]
pub fn capability_for(tool_id: &str) -> Capability {
    match tool_id {
        FS_READ | FS_LIST | FS_SEARCH => Capability::ReadFile,
        FS_WRITE => Capability::WriteFile,
        PROCESS_EXEC => Capability::ExecuteCommand,
        _ => Capability::SystemControl,
    }
}

/// Map a tool to its declared risk (mirrors the registered manifest).
#[must_use]
pub fn risk_for(tool_id: &str) -> (RiskLevel, bool) {
    match tool_id {
        FS_READ | FS_LIST | FS_SEARCH => (RiskLevel::Low, true),
        FS_WRITE => (RiskLevel::Medium, true),
        PROCESS_EXEC => (RiskLevel::High, false),
        _ => (RiskLevel::High, false),
    }
}

pub(crate) fn err(msg: &str) -> TalosError {
    TalosError::Storage(msg.to_owned())
}

/// Validate identifiers shared by every invocation.
pub fn validate_ids(tool_id: &str, task_id: &str, agent_id: &str) -> Result<()> {
    for (what, value) in [
        ("tool_id", tool_id),
        ("task_id", task_id),
        ("agent_id", agent_id),
    ] {
        if value.trim().is_empty() || value.len() > 128 {
            return Err(err(&format!("{what} must be 1..=128 chars")));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_and_risk_cover_builtins() {
        assert_eq!(capability_for(FS_READ), Capability::ReadFile);
        assert_eq!(capability_for(FS_WRITE), Capability::WriteFile);
        assert_eq!(capability_for(PROCESS_EXEC), Capability::ExecuteCommand);
        assert_eq!(risk_for(FS_READ), (RiskLevel::Low, true));
        assert_eq!(risk_for(PROCESS_EXEC), (RiskLevel::High, false));
    }

    #[test]
    fn ids_reject_empty() {
        assert!(validate_ids("", "t", "a").is_err());
        assert!(validate_ids(FS_READ, "t", "a").is_ok());
    }
}
