//! Policy Broker core types. Mirrors `@talos/contracts` policy domain.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Side-effect capability an agent wants to exercise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    /// Read a file.
    ReadFile,
    /// Write or modify a file.
    WriteFile,
    /// Run a shell command.
    ExecuteCommand,
    /// Open or drive a browser.
    OpenBrowser,
    /// Perform a UI automation action.
    UiAction,
    /// Issue a network request.
    NetworkRequest,
    /// Broad system control (settings, services, installs).
    SystemControl,
}

/// Blast radius of an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskLevel {
    /// Read-only or trivially scoped.
    Low,
    /// Bounded writes with clear scope.
    Medium,
    /// Wide or hard-to-reverse effects.
    High,
    /// Cannot be undone (data loss, external side effects).
    Irreversible,
}

/// A single agent action submitted for approval.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionRequest {
    /// Request identifier.
    pub id: String,
    /// Requesting agent.
    pub agent_id: String,
    /// Owning task.
    pub task_id: String,
    /// Capability being exercised.
    pub capability: Capability,
    /// Human-readable intent (1..=1024 chars).
    pub intent: String,
    /// Target of the action (path, URL, selector, ...).
    pub target: String,
    /// Opaque action payload (validated as JSON, never executed here).
    pub payload_json: serde_json::Value,
    /// Declared risk level.
    pub risk_level: RiskLevel,
    /// Whether the action can be undone.
    pub reversible: bool,
    /// When the action was requested.
    pub requested_at: DateTime<Utc>,
}

/// Verdict for an [`ActionRequest`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyDecision {
    /// Proceed without extra conditions.
    Allow,
    /// Block unconditionally.
    Deny,
    /// Proceed only after evidence is recorded.
    RequireEvidence,
    /// Proceed only with an approved compensation plan.
    RequireCompensation,
    /// Escalate to a human operator.
    RequireHumanReview,
}

/// The recorded outcome of evaluating one request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyEvaluation {
    /// The evaluated request's id.
    pub request_id: String,
    /// The verdict.
    pub decision: PolicyDecision,
    /// Human-readable rationale.
    pub reason: String,
    /// Follow-up checks the caller must satisfy (may be empty).
    pub required_checks: Vec<String>,
    /// When the evaluation was produced.
    pub created_at: DateTime<Utc>,
}

/// Outcome summary of the `--selftest` run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelftestReport {
    /// Total checks executed.
    pub total: usize,
    /// Checks that passed.
    pub passed: usize,
    /// `(name, ok, detail)` per check.
    pub checks: Vec<(String, bool, String)>,
}

impl SelftestReport {
    /// True when every check passed.
    #[must_use]
    pub fn ok(&self) -> bool {
        self.passed == self.total
    }
}
