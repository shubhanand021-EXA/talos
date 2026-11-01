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
/// Apply a unified diff patch to a file (dry-run validated, backup first).
pub const FS_EDIT: &str = "fs.edit";
/// Create a new file with content (fails when present unless overwritten).
pub const FS_CREATE: &str = "fs.create";
/// Delete a file (irreversible: backup + compensation plan required).
pub const FS_DELETE: &str = "fs.delete";
/// Move/rename a file (backup destination when overwritten).
pub const FS_MOVE: &str = "fs.move";
/// Run a workspace test suite (auto-detected framework, bounded output).
pub const CODE_RUN_TESTS: &str = "code.run_tests";
/// Install workspace dependencies (package-manager aware, network-gated).
pub const CODE_INSTALL_DEPS: &str = "code.install_deps";
/// Show workspace git status (porcelain, workspace-scoped).
pub const GIT_STATUS: &str = "git.status";
/// Show uncommitted workspace changes (stat + patch, bounded).
pub const GIT_DIFF: &str = "git.diff";
/// Show the workspace branch (workspace-scoped, read-only).
pub const GIT_BRANCH: &str = "git.branch";
/// Commit workspace changes (message validated, evidence-gated).
pub const GIT_COMMIT: &str = "git.commit";
/// Execute a subprocess (disabled by default, allowlisted, timed out).
pub const PROCESS_EXEC: &str = "process.exec";
/// Navigate the isolated browser to a URL (read-only planning by default).
pub const BROWSER_NAVIGATE: &str = "browser.navigate";
/// Click an element in the isolated browser (evidence-gated mutation).
pub const BROWSER_CLICK: &str = "browser.click";
/// Type text into an element (evidence-gated mutation).
pub const BROWSER_TYPE: &str = "browser.type";
/// Scroll the isolated browser viewport (reversible).
pub const BROWSER_SCROLL: &str = "browser.scroll";
/// Select an option in a form control (evidence-gated mutation).
pub const BROWSER_SELECT: &str = "browser.select";
/// Extract text from the isolated browser DOM (read-only).
pub const BROWSER_EXTRACT: &str = "browser.extract";
/// Capture a viewport screenshot hash (read-only, no pixels stored).
pub const BROWSER_SCREENSHOT: &str = "browser.screenshot";
/// Capture a DOM snapshot + hash (read-only).
pub const BROWSER_SNAPSHOT: &str = "browser.snapshot";
/// Manage the warm browser session (`launch` | `status` | `close`).
pub const BROWSER_SESSION: &str = "browser.session";
/// Move the pointer on the virtual screen (evidence-gated, reversible).
pub const COMPUTER_MOVE_MOUSE: &str = "computer.move_mouse";
/// Single click at coordinates or a selector (human-review gate).
pub const COMPUTER_CLICK: &str = "computer.click";
/// Double click at coordinates or a selector (human-review gate).
pub const COMPUTER_DOUBLE_CLICK: &str = "computer.double_click";
/// Press-drag-release gesture (human-review gate).
pub const COMPUTER_DRAG: &str = "computer.drag";
/// Type text, optionally at a focused element (human-review gate).
pub const COMPUTER_TYPE_TEXT: &str = "computer.type_text";
/// Press a hotkey chord (human-review gate).
pub const COMPUTER_HOTKEY: &str = "computer.hotkey";
/// Scroll at a position (evidence-gated, reversible).
pub const COMPUTER_SCROLL: &str = "computer.scroll";
/// Capture the virtual screen (read-only).
pub const COMPUTER_CAPTURE_SCREEN: &str = "computer.capture_screen";
/// Locate an element by selector (read-only).
pub const COMPUTER_LOCATE_ELEMENT: &str = "computer.locate_element";

/// All computer-use tool identifiers in registration order.
pub const COMPUTER_TOOLS: &[&str] = &[
    COMPUTER_MOVE_MOUSE,
    COMPUTER_CLICK,
    COMPUTER_DOUBLE_CLICK,
    COMPUTER_DRAG,
    COMPUTER_TYPE_TEXT,
    COMPUTER_HOTKEY,
    COMPUTER_SCROLL,
    COMPUTER_CAPTURE_SCREEN,
    COMPUTER_LOCATE_ELEMENT,
];

/// All built-in tool identifiers in registration order.
pub const BUILT_IN_TOOLS: &[&str] = &[
    FS_READ,
    FS_LIST,
    FS_SEARCH,
    FS_WRITE,
    FS_EDIT,
    FS_CREATE,
    FS_DELETE,
    FS_MOVE,
    PROCESS_EXEC,
    CODE_RUN_TESTS,
    CODE_INSTALL_DEPS,
    GIT_STATUS,
    GIT_DIFF,
    GIT_BRANCH,
    GIT_COMMIT,
    BROWSER_NAVIGATE,
    BROWSER_CLICK,
    BROWSER_TYPE,
    BROWSER_SCROLL,
    BROWSER_SELECT,
    BROWSER_EXTRACT,
    BROWSER_SCREENSHOT,
    BROWSER_SNAPSHOT,
    BROWSER_SESSION,
    COMPUTER_MOVE_MOUSE,
    COMPUTER_CLICK,
    COMPUTER_DOUBLE_CLICK,
    COMPUTER_DRAG,
    COMPUTER_TYPE_TEXT,
    COMPUTER_HOTKEY,
    COMPUTER_SCROLL,
    COMPUTER_CAPTURE_SCREEN,
    COMPUTER_LOCATE_ELEMENT,
];

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
        FS_WRITE | FS_EDIT | FS_CREATE | FS_MOVE | GIT_COMMIT => Capability::WriteFile,
        FS_DELETE => Capability::WriteFile,
        PROCESS_EXEC | CODE_RUN_TESTS | CODE_INSTALL_DEPS => Capability::ExecuteCommand,
        GIT_STATUS | GIT_DIFF | GIT_BRANCH => Capability::ReadFile,
        BROWSER_NAVIGATE | BROWSER_CLICK | BROWSER_TYPE | BROWSER_SCROLL | BROWSER_SELECT
        | BROWSER_EXTRACT | BROWSER_SCREENSHOT | BROWSER_SNAPSHOT | BROWSER_SESSION => {
            Capability::OpenBrowser
        }
        COMPUTER_MOVE_MOUSE
        | COMPUTER_CLICK
        | COMPUTER_DOUBLE_CLICK
        | COMPUTER_DRAG
        | COMPUTER_TYPE_TEXT
        | COMPUTER_HOTKEY
        | COMPUTER_SCROLL
        | COMPUTER_CAPTURE_SCREEN
        | COMPUTER_LOCATE_ELEMENT => Capability::UiAction,
        _ => Capability::SystemControl,
    }
}

/// Map a tool to its declared risk (mirrors the registered manifest).
#[must_use]
pub fn risk_for(tool_id: &str) -> (RiskLevel, bool) {
    match tool_id {
        FS_READ | FS_LIST | FS_SEARCH => (RiskLevel::Low, true),
        FS_WRITE | FS_EDIT | FS_CREATE | FS_MOVE => (RiskLevel::Medium, true),
        // Delete is destructive: compensation (backup) is mandatory.
        FS_DELETE => (RiskLevel::Irreversible, false),
        PROCESS_EXEC => (RiskLevel::High, true),
        // Tests run the workspace suite; reversible in the ledger sense and
        // capped like every other adapter.
        CODE_RUN_TESTS => (RiskLevel::Low, true),
        CODE_INSTALL_DEPS => (RiskLevel::Medium, true),
        GIT_STATUS | GIT_DIFF | GIT_BRANCH => (RiskLevel::Low, true),
        GIT_COMMIT => (RiskLevel::Medium, true),
        BROWSER_NAVIGATE | BROWSER_SCROLL | BROWSER_EXTRACT | BROWSER_SCREENSHOT
        | BROWSER_SNAPSHOT | BROWSER_SESSION => (RiskLevel::Low, true),
        BROWSER_CLICK | BROWSER_TYPE | BROWSER_SELECT => (RiskLevel::Medium, false),
        COMPUTER_CAPTURE_SCREEN | COMPUTER_LOCATE_ELEMENT => (RiskLevel::Low, true),
        COMPUTER_MOVE_MOUSE | COMPUTER_SCROLL => (RiskLevel::Medium, true),
        COMPUTER_CLICK
        | COMPUTER_DOUBLE_CLICK
        | COMPUTER_DRAG
        | COMPUTER_TYPE_TEXT
        | COMPUTER_HOTKEY => (RiskLevel::High, false),
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
        assert_eq!(capability_for(FS_EDIT), Capability::WriteFile);
        assert_eq!(capability_for(FS_DELETE), Capability::WriteFile);
        assert_eq!(capability_for(PROCESS_EXEC), Capability::ExecuteCommand);
        assert_eq!(capability_for(CODE_RUN_TESTS), Capability::ExecuteCommand);
        assert_eq!(capability_for(GIT_STATUS), Capability::ReadFile);
        assert_eq!(capability_for(GIT_COMMIT), Capability::WriteFile);
        assert_eq!(capability_for(BROWSER_CLICK), Capability::OpenBrowser);
        assert_eq!(capability_for(BROWSER_SNAPSHOT), Capability::OpenBrowser);
        assert_eq!(risk_for(FS_READ), (RiskLevel::Low, true));
        assert_eq!(risk_for(FS_EDIT), (RiskLevel::Medium, true));
        assert_eq!(risk_for(FS_DELETE), (RiskLevel::Irreversible, false));
        assert_eq!(risk_for(PROCESS_EXEC), (RiskLevel::High, true));
        assert_eq!(risk_for(CODE_RUN_TESTS), (RiskLevel::Low, true));
        assert_eq!(risk_for(GIT_COMMIT), (RiskLevel::Medium, true));
        assert_eq!(risk_for(BROWSER_CLICK), (RiskLevel::Medium, false));
        assert_eq!(risk_for(BROWSER_SNAPSHOT), (RiskLevel::Low, true));
    }

    #[test]
    fn ids_reject_empty() {
        assert!(validate_ids("", "t", "a").is_err());
        assert!(validate_ids(FS_READ, "t", "a").is_ok());
    }
}
