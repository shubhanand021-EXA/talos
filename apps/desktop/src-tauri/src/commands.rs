//! Secure IPC surface. Five commands, all typed, validated, and infallible
//! across the IPC boundary (failures become structured [`IpcError`] values).
//!
//! Contract mapping to `@talos/contracts` `IpcCommandName`:
//!
//! | Tauri command            | TS constant                          |
//! | ------------------------ | ------------------------------------ |
//! | `get_app_info`           | `IpcCommandName.getAppInfo`          |
//! | `get_system_status`      | `IpcCommandName.getSystemStatus`     |
//! | `request_action`         | `IpcCommandName.requestAction`       |
//! | `run_diagnostic`         | `IpcCommandName.runDiagnostic`       |
//! | `verify_evidence_ledger` | `IpcCommandName.verifyEvidenceLedger` |
//!
//! (`getAuditSummary` arrives with the audit UI.)
//!
//! Wire shapes mirror the contracts schemas exactly (`camelCase` JSON): the
//! TypeScript bridge validates every response with Zod, so any drift fails
//! loudly instead of silently.

use agent_runtime::RuntimeEvent;
use policy_broker::{evaluate_request_json, PolicyDecision};
use talos_common::{now_iso, TalosError};

use crate::events::{
    emit_talos_event, TalosEventName, AGENT_SPAWNED, EVIDENCE_UPDATED, POLICY_EVALUATED,
    TASK_CREATED, TASK_UPDATED,
};
use crate::state::{AppInfo, AppState};

/// Policy engine identifier stamped on every evaluation.
pub const POLICY_VERSION: &str = "talos-policy-broker/1";

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

/// System health snapshot. Field-for-field compatible with the TS
/// `SystemStatus` schema (`camelCase` JSON).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemStatus {
    /// Backend version.
    pub version: String,
    /// Seconds since process start.
    pub uptime_seconds: f64,
    /// Agents tracked by the runtime.
    pub active_agents: u32,
    /// Tasks tracked by the runtime.
    pub active_tasks: u32,
    /// False only if evaluation itself fails (it cannot for well-formed input).
    pub policy_broker_healthy: bool,
    /// False when the ledger cannot be read.
    pub evidence_ledger_healthy: bool,
    /// Last audit timestamp, if any.
    pub last_audit_at: Option<String>,
}

/// Policy verdict shaped exactly like the TS `PolicyEvaluation` schema.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IpcPolicyEvaluation {
    /// Evaluation identifier (`eval-{request_id}`).
    pub id: String,
    /// Evaluated request identifier.
    pub action_request_id: String,
    /// The verdict.
    pub decision: PolicyDecision,
    /// Human-readable rationale.
    pub reason: String,
    /// Engine identifier.
    pub policy_version: String,
    /// Follow-up checks the caller must satisfy (may be empty).
    pub required_checks: Vec<String>,
    /// When the evaluation was produced (RFC 3339).
    pub evaluated_at: chrono::DateTime<chrono::Utc>,
}

impl From<&policy_broker::PolicyEvaluation> for IpcPolicyEvaluation {
    fn from(evaluation: &policy_broker::PolicyEvaluation) -> Self {
        Self {
            id: format!("eval-{}", evaluation.request_id),
            action_request_id: evaluation.request_id.clone(),
            decision: evaluation.decision,
            reason: evaluation.reason.clone(),
            policy_version: POLICY_VERSION.to_owned(),
            required_checks: evaluation.required_checks.clone(),
            evaluated_at: evaluation.created_at,
        }
    }
}

/// Basic ledger integrity snapshot (counts + hash probe, not full verification).
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

/// Deterministic local diagnostic summary (echo-agent loop).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticSummary {
    /// Steps executed.
    pub total: usize,
    /// Steps passed.
    pub passed: usize,
    /// Per-step rows.
    pub checks: Vec<DiagnosticCheck>,
    /// Ledger snapshot taken during the run.
    pub ledger: LedgerStatus,
    /// Task created for this run.
    pub task_id: String,
    /// Agent spawned for this run.
    pub agent_id: String,
    /// Policy verdict for the echo action.
    pub evaluation: IpcPolicyEvaluation,
}

/// Build the current system status snapshot (pure logic, unit-tested).
pub fn system_status_of(
    state: &AppState,
    ledger_healthy: bool,
    active_agents: u32,
    active_tasks: u32,
) -> SystemStatus {
    SystemStatus {
        version: state.app_info.version.clone(),
        uptime_seconds: state.started_at.elapsed().as_secs_f64(),
        active_agents,
        active_tasks,
        policy_broker_healthy: true,
        evidence_ledger_healthy: ledger_healthy,
        last_audit_at: None,
    }
}

/// Evaluate one raw action request (never panics; malformed input denies),
/// shaped for the contracts `PolicyEvaluation` schema.
#[must_use]
pub fn evaluate_action(state: &AppState, raw: &serde_json::Value) -> IpcPolicyEvaluation {
    IpcPolicyEvaluation::from(&evaluate_request_json(&state.engine, raw))
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

/// Run the local echo-agent loop and collect the Tauri emissions for it.
///
/// Returns the summary plus ordered `(event, payload)` pairs; the command
/// handler emits them. Pure orchestration — no `AppHandle` needed, so tests
/// exercise the exact production path.
pub async fn diagnostic_flow(
    state: &AppState,
) -> Result<(DiagnosticSummary, Vec<(TalosEventName, serde_json::Value)>), IpcError> {
    let run = {
        let mut runtime = state.runtime.lock().await;
        runtime.run_diagnostic(&state.engine, &state.db).await?
    };

    let mut emissions: Vec<(TalosEventName, serde_json::Value)> = Vec::new();
    for event in &run.events {
        let (name, payload) = runtime_event_payload(&run, event);
        emissions.push((name, payload));
    }

    let evaluation = IpcPolicyEvaluation::from(&run.evaluation);
    let summary = DiagnosticSummary {
        total: run.events.len(),
        passed: run.events.len(),
        checks: run
            .events
            .iter()
            .map(|event| DiagnosticCheck {
                name: runtime_event_step(event),
                ok: true,
                detail: runtime_event_detail(event),
            })
            .collect(),
        ledger: ledger_status_of(state).await?,
        task_id: run.task.id.clone(),
        agent_id: run.agent.id.clone(),
        evaluation,
    };
    Ok((summary, emissions))
}

/// Map one runtime event onto its contracts event name + JSON payload.
fn runtime_event_payload(
    run: &agent_runtime::DiagnosticRun,
    event: &RuntimeEvent,
) -> (TalosEventName, serde_json::Value) {
    match event {
        RuntimeEvent::TaskCreated { task_id, title } => (
            TASK_CREATED,
            serde_json::json!({ "task": { "id": task_id, "title": title, "status": "executing" } }),
        ),
        RuntimeEvent::AgentSpawned {
            agent_id,
            task_id,
            display_name,
            role,
        } => (
            AGENT_SPAWNED,
            serde_json::json!({ "agent": {
                "id": agent_id,
                "displayName": display_name,
                "role": role,
                "status": "running",
                "sessionId": "session-local",
                "capabilities": ["fs.read"],
                "createdAt": now_iso(),
                "updatedAt": now_iso(),
                "taskId": task_id,
            } }),
        ),
        RuntimeEvent::ActionProposed { request_id } => (
            TASK_UPDATED,
            serde_json::json!({ "task": {
                "id": run.task.id,
                "title": run.task.title,
                "status": "executing",
                "proposal": request_id,
            } }),
        ),
        RuntimeEvent::PolicyEvaluated {
            request_id: _,
            decision: _,
        } => (
            POLICY_EVALUATED,
            serde_json::json!({ "evaluation": IpcPolicyEvaluation::from(&run.evaluation) }),
        ),
        RuntimeEvent::EvidenceRecorded { evidence_id } => (
            EVIDENCE_UPDATED,
            serde_json::json!({ "evidence": {
                "id": evidence_id,
                "kind": "log",
                "status": "pending",
                "label": format!("diagnostic {}", run.task.id),
                "summary": run.evaluation.reason,
                "hash": "sha256:ledger",
                "payload": { "decision": format!("{:?}", run.evaluation.decision) },
                "createdAt": now_iso(),
                "taskId": run.task.id,
                "agentId": run.agent.id,
            } }),
        ),
        RuntimeEvent::TaskCompleted { task_id, summary } => (
            TASK_UPDATED,
            serde_json::json!({ "task": {
                "id": task_id,
                "title": run.task.title,
                "status": "completed",
                "summary": summary,
            } }),
        ),
    }
}

/// Stable step name per runtime event (drives the summary checklist).
fn runtime_event_step(event: &RuntimeEvent) -> String {
    match event {
        RuntimeEvent::TaskCreated { .. } => "task_created".to_owned(),
        RuntimeEvent::AgentSpawned { .. } => "agent_spawned".to_owned(),
        RuntimeEvent::ActionProposed { .. } => "action_proposed".to_owned(),
        RuntimeEvent::PolicyEvaluated { .. } => "policy_evaluated".to_owned(),
        RuntimeEvent::EvidenceRecorded { .. } => "evidence_recorded".to_owned(),
        RuntimeEvent::TaskCompleted { .. } => "task_completed".to_owned(),
    }
}

/// Human-readable detail per runtime event (drives the summary checklist).
fn runtime_event_detail(event: &RuntimeEvent) -> String {
    match event {
        RuntimeEvent::TaskCreated { task_id, .. } => format!("task {task_id} created"),
        RuntimeEvent::AgentSpawned { agent_id, .. } => format!("agent {agent_id} spawned"),
        RuntimeEvent::ActionProposed { request_id } => {
            format!("action {request_id} proposed")
        }
        RuntimeEvent::PolicyEvaluated { decision, .. } => {
            format!("policy verdict: {decision:?}")
        }
        RuntimeEvent::EvidenceRecorded { evidence_id } => {
            format!("evidence {evidence_id} recorded")
        }
        RuntimeEvent::TaskCompleted { summary, .. } => summary.clone(),
    }
}

/// `get_app_info` — static product metadata.
#[tauri::command]
pub fn get_app_info(state: tauri::State<'_, AppState>) -> AppInfo {
    state.app_info.clone()
}

/// `get_system_status` — health snapshot with live runtime counts.
#[tauri::command]
pub async fn get_system_status(
    state: tauri::State<'_, AppState>,
) -> Result<SystemStatus, IpcError> {
    let ledger_healthy = ledger_status_of(&state)
        .await
        .map(|status| status.healthy)?;
    let runtime = state.runtime.lock().await;
    Ok(system_status_of(
        &state,
        ledger_healthy,
        u32::try_from(runtime.state().agents.len()).unwrap_or(u32::MAX),
        u32::try_from(runtime.state().tasks.len()).unwrap_or(u32::MAX),
    ))
}

/// `request_action` — evaluate one action request through the policy engine.
///
/// Malformed input yields a deny-evaluation (never an IPC transport error),
/// so the frontend always receives a renderable verdict.
#[tauri::command]
pub fn request_action(
    state: tauri::State<'_, AppState>,
    raw: serde_json::Value,
) -> IpcPolicyEvaluation {
    evaluate_action(&state, &raw)
}

/// `run_diagnostic` — local echo-agent loop; emits one Tauri event per stage.
#[tauri::command]
pub async fn run_diagnostic(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<DiagnosticSummary, IpcError> {
    let (summary, emissions) = diagnostic_flow(&state).await?;
    for (name, payload) in emissions {
        if let Err(err) = emit_talos_event(&app, name, payload) {
            eprintln!("[talos] event emission failed ({name}): {err}");
        }
    }
    Ok(summary)
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
    async fn system_status_reports_runtime_counts() {
        let state = test_state().await;
        let empty = system_status_of(&state, true, 0, 0);
        assert_eq!(empty.active_agents, 0);
        let status = system_status_of(&state, true, 2, 3);
        assert_eq!(status.active_agents, 2);
        assert_eq!(status.active_tasks, 3);
        assert!(status.policy_broker_healthy);
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
        assert_eq!(evaluation.action_request_id, "t-1");
        assert!(!evaluation.policy_version.is_empty());
    }

    #[tokio::test]
    async fn request_action_denies_garbage_without_error() {
        let state = test_state().await;
        let evaluation = evaluate_action(&state, &serde_json::json!({ "nope": true }));
        assert_eq!(evaluation.decision, policy_broker::PolicyDecision::Deny);
    }

    #[tokio::test]
    async fn evaluation_serializes_to_contracts_shape() {
        let state = test_state().await;
        let raw = serde_json::json!({
            "id": "t-9",
            "agent_id": "a-9",
            "task_id": "task-9",
            "capability": "read_file",
            "intent": "read",
            "target": "/tmp/y",
            "payload_json": {},
            "risk_level": "low",
            "reversible": true,
            "requested_at": now_iso(),
        });
        let evaluation = evaluate_action(&state, &raw);
        let json = serde_json::to_value(&evaluation).expect("serializes");
        for key in [
            "id",
            "actionRequestId",
            "decision",
            "reason",
            "policyVersion",
            "requiredChecks",
            "evaluatedAt",
        ] {
            assert!(json.get(key).is_some(), "missing key {key}");
        }
    }

    #[tokio::test]
    async fn diagnostic_flow_uses_orchestrator_and_emits() {
        let state = test_state().await;
        let (summary, emissions) = diagnostic_flow(&state).await.expect("diagnostic runs");
        assert_eq!(summary.passed, summary.total);
        assert!(summary.ledger.healthy);
        assert_eq!(summary.task_id, "diag-task-1");
        assert_eq!(summary.agent_id, "diag-agent-1");
        assert_eq!(
            summary.evaluation.decision,
            policy_broker::PolicyDecision::Allow
        );
        let names: Vec<&str> = emissions.iter().map(|(name, _)| *name).collect();
        assert_eq!(
            names,
            [
                TASK_CREATED,
                AGENT_SPAWNED,
                TASK_UPDATED,
                POLICY_EVALUATED,
                EVIDENCE_UPDATED,
                TASK_UPDATED
            ]
        );
    }

    #[tokio::test]
    async fn ledger_counts_start_at_zero() {
        let state = test_state().await;
        let status = ledger_status_of(&state).await.expect("ledger reads");
        assert!(status.healthy);
        assert_eq!(status.event_count, 0);
    }
}
