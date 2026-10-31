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
