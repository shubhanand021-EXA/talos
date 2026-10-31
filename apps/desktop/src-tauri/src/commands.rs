//! Secure IPC surface. Five commands, all typed, validated, and infallible
//! across the IPC boundary (failures become structured [`IpcError`] values).
//!
//! Contract mapping to `@talos/contracts` `IpcCommandName`:
//!
//! | Tauri command          | TS constant                          |
//! | ---------------------- | ------------------------------------ |
//! | `get_app_info`         | `IpcCommandName.getAppInfo`          |
//! | `get_system_status`    | `IpcCommandName.getSystemStatus`     |
//! | `request_action`       | `IpcCommandName.requestAction`       |
//! | `run_diagnostic`       | `IpcCommandName.runDiagnostic`       |
//! | `verify_evidence_ledger` | `IpcCommandName.verifyEvidenceLedger` |
//!
//! (`getAuditSummary` arrives with the audit UI.)

use policy_broker::{evaluate_request_json, PolicyEvaluation};
use talos_common::{now_iso, TalosError};

use crate::state::{AppInfo, AppState};

/// Structured IPC error: machine-readable `code` plus human `message`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IpcError {
    /// Stable error code (e.g. `storage_unavailable`).
    pub code: String,
    /// Human-readable detail.
    pub message: String,
}

impl From<TalosError> for IpcError {
    fn from(err: TalosError) -> Self {
        let code = match err {
            TalosError::InvalidId { .. } => "invalid_id",
            TalosError::Serialization(_) => "serialization",
            TalosError::Storage(_) => "storage_unavailable",
            TalosError::Policy(_) => "policy_failure",
            TalosError::Time(_) => "time_failure",
            TalosError::Internal(_) => "internal",
        };
        Self {
            code: code.to_owned(),
            message: err.to_string(),
        }
    }
}

/// System health snapshot. Mirrors the TS `SystemStatus` schema.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SystemStatus {
    /// Backend version.
    pub version: String,
    /// Seconds since process start.
    pub uptime_seconds: f64,
    /// Agents currently tracked (0 until the runtime lands).
    pub active_agents: u32,
    /// Tasks currently tracked (0 until the runtime lands).
    pub active_tasks: u32,
    /// False only if evaluation itself fails (it cannot for well-formed input).
    pub policy_broker_healthy: bool,
    /// False when the ledger cannot be read.
    pub evidence_ledger_healthy: bool,
    /// Last audit timestamp, if any.
    pub last_audit_at: Option<String>,
}

/// Basic ledger integrity snapshot (counts + hash presence, not full verification).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LedgerStatus {
    /// Whether every integrity probe passed.
    pub healthy: bool,
    /// Rows in `events`.
    pub event_count: i64,
    /// Rows in `evidence_records`.
    pub evidence_count: i64,
    /// Rows in `claims`.
    pub claim_count: i64,
    /// Human-readable note (empty-hash count, null timestamps, ...).
    pub detail: String,
    /// When the check ran.
    pub checked_at: String,
}

/// One deterministic diagnostic step.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DiagnosticCheck {
    /// Step name.
    pub name: String,
    /// Whether the step passed.
    pub ok: bool,
    /// Detail line.
    pub detail: String,
}

/// Deterministic local diagnostic summary.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DiagnosticSummary {
    /// Steps executed.
    pub total: usize,
    /// Steps passed.
    pub passed: usize,
    /// Per-step rows.
    pub checks: Vec<DiagnosticCheck>,
    /// Ledger snapshot taken during the run.
    pub ledger: LedgerStatus,
}

/// Build the current system status snapshot (pure logic, unit-tested).
pub fn system_status_of(state: &AppState, ledger_healthy: bool) -> SystemStatus {
    SystemStatus {
        version: state.app_info.version.clone(),
        uptime_seconds: state.started_at.elapsed().as_secs_f64(),
        active_agents: 0,
        active_tasks: 0,
        policy_broker_healthy: true,
        evidence_ledger_healthy: ledger_healthy,
        last_audit_at: None,
    }
}

/// Evaluate one raw action request (never panics; malformed input denies).
#[must_use]
pub fn evaluate_action(state: &AppState, raw: &serde_json::Value) -> PolicyEvaluation {
    evaluate_request_json(&state.engine, raw)
}

/// Count ledger rows and probe hash presence (pure logic, unit-tested).
pub async fn ledger_status_of(state: &AppState) -> Result<LedgerStatus, IpcError> {
    let pool = state.db.pool();
    let event_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events")
        .fetch_one(pool)
        .await
        .map_err(|err| IpcError::from(TalosError::Storage(err.to_string())))?;
    let evidence_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM evidence_records")
        .fetch_one(pool)
        .await
        .map_err(|err| IpcError::from(TalosError::Storage(err.to_string())))?;
    let claim_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM claims")
        .fetch_one(pool)
        .await
        .map_err(|err| IpcError::from(TalosError::Storage(err.to_string())))?;
    let empty_hashes: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM evidence_records WHERE hash IS NULL OR hash = ''")
            .fetch_one(pool)
            .await
            .map_err(|err| IpcError::from(TalosError::Storage(err.to_string())))?;
    let healthy = empty_hashes == 0;
    Ok(LedgerStatus {
        healthy,
        event_count,
        evidence_count,
        claim_count,
        detail: if healthy {
            format!("{evidence_count} evidence rows carry integrity hashes")
        } else {
            format!("{empty_hashes} evidence rows lack integrity hashes")
        },
        checked_at: now_iso(),
    })
}

/// Deterministic diagnostic flow: sample task, policy probe, ledger writes.
pub async fn diagnostic_flow(state: &AppState) -> Result<DiagnosticSummary, IpcError> {
    let mut checks: Vec<DiagnosticCheck> = Vec::new();

    // Step 1 — policy probe: low-risk read must allow.
    let probe = serde_json::json!({
        "id": "diag-probe-read",
        "agent_id": "diag-agent",
        "task_id": "diag-task",
        "capability": "read_file",
        "intent": "diagnostic read probe",
        "target": "/tmp/talos-diag.txt",
        "payload_json": {},
        "risk_level": "low",
        "reversible": true,
        "requested_at": now_iso(),
    });
    let evaluation = evaluate_action(state, &probe);
    checks.push(DiagnosticCheck {
        name: "policy_probe_read".to_owned(),
        ok: evaluation.decision == policy_broker::PolicyDecision::Allow,
        detail: format!("decision={:?}", evaluation.decision),
    });

    // Step 2 — write probe must gate behind evidence.
    let write_probe = serde_json::json!({
        "id": "diag-probe-write",
        "agent_id": "diag-agent",
        "task_id": "diag-task",
        "capability": "write_file",
        "intent": "diagnostic write probe",
        "target": "/tmp/talos-diag.txt",
        "payload_json": {},
        "risk_level": "medium",
        "reversible": true,
        "requested_at": now_iso(),
    });
    let write_eval = evaluate_action(state, &write_probe);
    checks.push(DiagnosticCheck {
        name: "policy_probe_write".to_owned(),
        ok: write_eval.decision == policy_broker::PolicyDecision::RequireEvidence,
        detail: format!("decision={:?}", write_eval.decision),
    });

    // Step 3 — ledger write round-trip.
    let event_result = evidence_store::insert_event(
        &state.db,
        evidence_store::NewEvent {
            id: format!("diag-evt-{}", state.started_at.elapsed().as_millis()),
            kind: "diagnostic".to_owned(),
            payload_json: serde_json::json!({ "at": now_iso() }),
        },
    )
    .await;
    checks.push(DiagnosticCheck {
        name: "ledger_write".to_owned(),
        ok: event_result.is_ok(),
        detail: match &event_result {
            Ok(_) => "event appended".to_owned(),
            Err(err) => err.to_string(),
        },
    });

    let ledger = ledger_status_of(state).await?;
    let passed = checks.iter().filter(|check| check.ok).count();
    Ok(DiagnosticSummary {
        total: checks.len(),
        passed,
        checks,
        ledger,
    })
}

/// `get_app_info` — static product metadata.
#[tauri::command]
pub fn get_app_info(state: tauri::State<'_, AppState>) -> AppInfo {
    state.app_info.clone()
}

/// `get_system_status` — deterministic health snapshot.
#[tauri::command]
pub async fn get_system_status(
    state: tauri::State<'_, AppState>,
) -> Result<SystemStatus, IpcError> {
    let ledger_healthy = ledger_status_of(&state)
        .await
        .map(|status| status.healthy)?;
    Ok(system_status_of(&state, ledger_healthy))
}

/// `request_action` — evaluate one action request through the policy engine.
///
/// Malformed input yields a deny-evaluation (never an IPC transport error),
/// so the frontend always receives a renderable verdict.
#[tauri::command]
pub fn request_action(
    state: tauri::State<'_, AppState>,
    raw: serde_json::Value,
) -> PolicyEvaluation {
    evaluate_action(&state, &raw)
}

/// `run_diagnostic` — deterministic local diagnostic flow.
#[tauri::command]
pub async fn run_diagnostic(
    state: tauri::State<'_, AppState>,
) -> Result<DiagnosticSummary, IpcError> {
    diagnostic_flow(&state).await
}

/// `verify_evidence_ledger` — basic integrity snapshot (counts + hash probe).
#[tauri::command]
pub async fn verify_evidence_ledger(
    state: tauri::State<'_, AppState>,
) -> Result<LedgerStatus, IpcError> {
    ledger_status_of(&state).await
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_state() -> AppState {
        AppState::new_in_memory()
            .await
            .expect("in-memory state builds")
    }

    #[tokio::test]
    async fn system_status_is_deterministic() {
        let state = test_state().await;
        let status = system_status_of(&state, true);
        assert_eq!(status.version, "0.1.0");
        assert!(status.policy_broker_healthy);
        assert_eq!(status.active_agents, 0);
    }

    #[tokio::test]
    async fn request_action_allows_low_risk_read() {
        let state = test_state().await;
        let raw = serde_json::json!({
            "id": "t-1",
            "agent_id": "a-1",
            "task_id": "task-1",
            "capability": "read_file",
            "intent": "read",
            "target": "/tmp/x",
            "payload_json": {},
            "risk_level": "low",
            "reversible": true,
            "requested_at": now_iso(),
        });
        let evaluation = evaluate_action(&state, &raw);
        assert_eq!(evaluation.decision, policy_broker::PolicyDecision::Allow);
    }

    #[tokio::test]
    async fn request_action_denies_garbage_without_error() {
        let state = test_state().await;
        let evaluation = evaluate_action(&state, &serde_json::json!({ "nope": true }));
        assert_eq!(evaluation.decision, policy_broker::PolicyDecision::Deny);
    }

    #[tokio::test]
    async fn diagnostic_flow_passes() {
        let state = test_state().await;
        let summary = diagnostic_flow(&state).await.expect("diagnostic runs");
        assert_eq!(summary.passed, summary.total);
        assert!(summary.ledger.healthy);
    }

    #[tokio::test]
    async fn ledger_counts_start_at_zero() {
        let state = test_state().await;
        let status = ledger_status_of(&state).await.expect("ledger reads");
        assert!(status.healthy);
        assert_eq!(status.event_count, 0);
    }
}
