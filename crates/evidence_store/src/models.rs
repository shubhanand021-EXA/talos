//! Row types shared by the repository layer.

use serde::{Deserialize, Serialize};

/// Full `events` row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::FromRow)]
pub struct EventRow {
    /// Event identifier.
    pub id: String,
    /// Event kind (e.g. `task_created`).
    pub kind: String,
    /// Raw JSON payload.
    pub payload_json: String,
    /// RFC 3339 creation timestamp.
    pub created_at: String,
}

/// Payload for [`crate::insert_event`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewEvent {
    /// Event identifier.
    pub id: String,
    /// Event kind.
    pub kind: String,
    /// JSON payload (stored verbatim).
    pub payload_json: serde_json::Value,
}

/// Full `evidence_records` row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::FromRow)]
pub struct EvidenceRow {
    /// Evidence identifier.
    pub id: String,
    /// Owning task, if any.
    pub task_id: Option<String>,
    /// Producing agent, if any.
    pub agent_id: Option<String>,
    /// Owning session, if any.
    pub session_id: Option<String>,
    /// Evidence kind (mirrors the TS `EvidenceKind` union).
    pub kind: String,
    /// Verification status.
    pub status: String,
    /// Short label.
    pub label: Option<String>,
    /// Longer summary.
    pub summary: Option<String>,
    /// Integrity hash (`sha256:` hex of the payload).
    pub hash: String,
    /// Optional external location.
    pub uri: Option<String>,
    /// Optional MIME type.
    pub mime_type: Option<String>,
    /// Raw JSON payload, if any.
    pub payload_json: Option<String>,
    /// RFC 3339 creation timestamp.
    pub created_at: String,
    /// RFC 3339 verification timestamp, if verified.
    pub verified_at: Option<String>,
    /// Position in the evidence hash chain (`NULL` for pre-chain legacy rows).
    pub seq: Option<i64>,
    /// Previous chain hash (`NULL` for pre-chain legacy rows).
    pub prev_hash: Option<String>,
}

/// Payload for [`crate::insert_evidence`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewEvidence {
    /// Evidence identifier.
    pub id: String,
    /// Owning task, if any.
    pub task_id: Option<String>,
    /// Producing agent, if any.
    pub agent_id: Option<String>,
    /// Owning session, if any.
    pub session_id: Option<String>,
    /// Evidence kind.
    pub kind: String,
    /// Short label.
    pub label: Option<String>,
    /// Longer summary.
    pub summary: Option<String>,
    /// Optional external location.
    pub uri: Option<String>,
    /// Optional MIME type.
    pub mime_type: Option<String>,
    /// JSON payload to hash and store.
    pub payload: serde_json::Value,
}

/// Full `claims` row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::FromRow)]
pub struct ClaimRow {
    /// Claim identifier.
    pub id: String,
    /// Human-readable claim statement.
    pub statement: String,
    /// JSON array of evidence IDs backing the claim.
    pub evidence_ids_json: String,
    /// Agent that raised the claim, if any.
    pub created_by_agent_id: Option<String>,
    /// Owning task, if any.
    pub task_id: Option<String>,
    /// RFC 3339 creation timestamp.
    pub created_at: String,
    /// Lifecycle status: `pending` | `verified` | `rejected` | `conflicting`.
    pub status: String,
}

/// Payload for [`crate::insert_claim`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewClaim {
    /// Claim identifier.
    pub id: String,
    /// Human-readable claim statement.
    pub statement: String,
    /// Evidence IDs backing the claim (must be non-empty).
    pub evidence_ids: Vec<String>,
    /// Agent that raised the claim, if any.
    pub created_by_agent_id: Option<String>,
    /// Owning task, if any.
    pub task_id: Option<String>,
}

/// Payload for [`crate::create_claim`]: a bare statement that may not yet
/// have any supporting evidence (`status` starts as `pending`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewClaimPending {
    /// Claim identifier.
    pub id: String,
    /// Human-readable claim statement.
    pub statement: String,
    /// Agent that raised the claim, if any.
    pub created_by_agent_id: Option<String>,
    /// Owning task, if any.
    pub task_id: Option<String>,
}

/// Full `event_log` row (hash-chained).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::FromRow)]
pub struct EventLogRow {
    /// Chain position (1-based, contiguous).
    pub seq: i64,
    /// Event identifier.
    pub id: String,
    /// Event kind (e.g. `task_created`).
    pub kind: String,
    /// Raw JSON payload.
    pub payload_json: String,
    /// Hash of the previous entry (`GENESIS` for `seq = 1`).
    pub prev_hash: String,
    /// `sha256:` chain hash over seq, id, kind, payload, prev hash, timestamp.
    pub hash: String,
    /// RFC 3339 creation timestamp.
    pub created_at: String,
}

/// Full `action_requests` row (policy audit trail).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::FromRow)]
pub struct ActionRequestRow {
    /// Request identifier.
    pub id: String,
    /// Requesting agent.
    pub agent_id: String,
    /// Owning task.
    pub task_id: String,
    /// Capability exercised.
    pub capability: String,
    /// Human-readable intent.
    pub intent: String,
    /// Action target.
    pub target: String,
    /// Raw JSON payload.
    pub payload_json: String,
    /// Declared risk level.
    pub risk_level: String,
    /// Whether the action is reversible (0/1).
    pub reversible: i64,
    /// RFC 3339 request timestamp.
    pub requested_at: String,
}

/// Payload for [`crate::record_action_request`]. Plain strings — the
/// evidence store never depends on the policy crate (no dependency cycle).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewActionRecord {
    /// Request identifier.
    pub id: String,
    /// Requesting agent.
    pub agent_id: String,
    /// Owning task.
    pub task_id: String,
    /// Capability exercised.
    pub capability: String,
    /// Human-readable intent.
    pub intent: String,
    /// Action target.
    pub target: String,
    /// Raw JSON payload.
    pub payload: serde_json::Value,
    /// Declared risk level.
    pub risk_level: String,
    /// Whether the action is reversible.
    pub reversible: bool,
    /// RFC 3339 request timestamp.
    pub requested_at: String,
}

/// Full `policy_evaluations` row (policy audit trail).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::FromRow)]
pub struct PolicyEvaluationRow {
    /// Evaluation identifier.
    pub id: String,
    /// Evaluated request identifier.
    pub action_request_id: String,
    /// Verdict.
    pub decision: String,
    /// Human-readable rationale.
    pub reason: String,
    /// Engine version string.
    pub policy_version: String,
    /// JSON array of follow-up checks.
    pub required_checks_json: String,
    /// RFC 3339 evaluation timestamp.
    pub evaluated_at: String,
}

/// Full `compensation_plans` row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::FromRow)]
pub struct CompensationPlanRow {
    /// Plan identifier.
    pub id: String,
    /// Action request this plan covers.
    pub action_request_id: String,
    /// Recovery strategy.
    pub strategy: String,
    /// Lifecycle: `proposed` | `approved` | `executed` | `failed` | `rolled_back`.
    pub status: String,
    /// Why the plan exists.
    pub reason: String,
    /// RFC 3339 creation timestamp.
    pub created_at: String,
    /// RFC 3339 last-transition timestamp.
    pub updated_at: String,
}

/// Payload for [`crate::create_compensation_plan`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewCompensationPlan {
    /// Plan identifier.
    pub id: String,
    /// Action request this plan covers.
    pub action_request_id: String,
    /// Recovery strategy.
    pub strategy: String,
    /// Why the plan exists.
    pub reason: String,
    /// Ordered recovery steps (at least one).
    pub steps: Vec<NewCompensationStep>,
}

/// Payload for one plan step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewCompensationStep {
    /// Recovery action type (e.g. `rollback_command`).
    pub action_type: String,
    /// Step payload as JSON.
    pub payload: serde_json::Value,
}

/// Full `compensation_steps` row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::FromRow)]
pub struct CompensationStepRow {
    /// Step identifier.
    pub id: String,
    /// Owning plan identifier.
    pub plan_id: String,
    /// Zero-based execution order.
    pub step_index: i64,
    /// Recovery action type.
    pub action_type: String,
    /// Raw JSON payload.
    pub payload_json: String,
    /// Step status: `pending` | `in_progress` | `done` | `failed` | `skipped`.
    pub status: String,
}
