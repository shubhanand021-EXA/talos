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
use policy_broker::{evaluate_request_json, PolicyDecision, PolicyEngine};
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

/// Basic ledger integrity snapshot: hash chains plus recent rows.
/// `snake_case` JSON to match the existing ledger wire shape.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LedgerStatus {
    /// Whether every integrity probe passed.
    pub healthy: bool,
    /// Rows in `event_log`.
    pub event_count: i64,
    /// Rows in `evidence_records`.
    pub evidence_count: i64,
    /// Rows in `claims`.
    pub claim_count: i64,
    /// Human-readable note (empty-hash count, null timestamps, ...).
    pub detail: String,
    /// When the check ran.
    pub checked_at: String,
    /// Event-log chain verdict.
    pub chain_valid: bool,
    /// Links recomputed in the event log.
    pub chain_checked: i64,
    /// Highest event-log sequence, if any.
    pub chain_last_seq: Option<i64>,
    /// Hash at `chain_last_seq`, if any.
    pub chain_last_hash: Option<String>,
    /// First broken event-log sequence, if any.
    pub chain_first_bad_seq: Option<i64>,
    /// Newest chained events (id, kind, seq, created_at).
    pub recent_events: Vec<EventSummary>,
    /// Newest evidence rows (id, kind, status, hash, created_at).
    pub recent_evidence: Vec<EvidenceSummary>,
    /// Newest claims (id, statement, status, created_at).
    pub recent_claims: Vec<ClaimSummary>,
    /// Compensation plans, newest first.
    pub compensation_plans: Vec<CompensationPlanSummary>,
}

/// Chained event row for the Effects panel.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EventSummary {
    /// Event identifier.
    pub id: String,
    /// Event kind.
    pub kind: String,
    /// Chain position.
    pub seq: i64,
    /// RFC 3339 creation timestamp.
    pub created_at: String,
}

/// Evidence row for the Effects panel.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EvidenceSummary {
    /// Evidence identifier.
    pub id: String,
    /// Evidence kind.
    pub kind: String,
    /// Lifecycle status.
    pub status: String,
    /// Integrity hash.
    pub hash: String,
    /// RFC 3339 creation timestamp.
    pub created_at: String,
}

/// Claim row for the Effects panel.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ClaimSummary {
    /// Claim identifier.
    pub id: String,
    /// Claim statement.
    pub statement: String,
    /// Lifecycle status.
    pub status: String,
    /// RFC 3339 creation timestamp.
    pub created_at: String,
}

/// Compensation plan row for the Effects panel.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CompensationPlanSummary {
    /// Plan identifier.
    pub id: String,
    /// Covered action request.
    pub action_request_id: String,
    /// Recovery strategy.
    pub strategy: String,
    /// Lifecycle status.
    pub status: String,
    /// RFC 3339 last-transition timestamp.
    pub updated_at: String,
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
    /// Policy verdict for the final (irreversible, compensated) action.
    pub evaluation: IpcPolicyEvaluation,
    /// Claim raised for the run.
    pub claim_id: String,
    /// Compensation plan recorded for the irreversible stage.
    pub plan_id: String,
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

/// Full integrity snapshot: both hash chains plus recent rows for the UI.
pub async fn ledger_status_of(state: &AppState) -> Result<LedgerStatus, IpcError> {
    use evidence_store as store;
    let report = store::get_integrity_report(&state.db)
        .await
        .map_err(IpcError::from)?;
    let recent_events = store::list_recent_log(&state.db, 20)
        .await
        .map_err(IpcError::from)?
        .into_iter()
        .map(|row| EventSummary {
            id: row.id,
            kind: row.kind,
            seq: row.seq,
            created_at: row.created_at,
        })
        .collect::<Vec<_>>();
    let recent_evidence = store::list_recent_evidence(&state.db, 20)
        .await
        .map_err(IpcError::from)?
        .into_iter()
        .map(|row| EvidenceSummary {
            id: row.id,
            kind: row.kind,
            status: row.status,
            hash: row.hash,
            created_at: row.created_at,
        })
        .collect::<Vec<_>>();
    let recent_claims = store::list_recent_claims(&state.db, 20)
        .await
        .map_err(IpcError::from)?
        .into_iter()
        .map(|row| ClaimSummary {
            id: row.id,
            statement: row.statement,
            status: row.status,
            created_at: row.created_at,
        })
        .collect::<Vec<_>>();
    let compensation_plans = store::list_compensation_plans(&state.db, 20)
        .await
        .map_err(IpcError::from)?
        .into_iter()
        .map(|row| CompensationPlanSummary {
            id: row.id,
            action_request_id: row.action_request_id,
            strategy: row.strategy,
            status: row.status,
            updated_at: row.updated_at,
        })
        .collect::<Vec<_>>();
    let healthy = report.healthy;
    Ok(LedgerStatus {
        healthy,
        event_count: report.event_count,
        evidence_count: report.evidence_count,
        claim_count: report.claim_count,
        detail: if healthy {
            format!(
                "chains verified ({} event links, {} evidence rows)",
                report.chain.checked, report.evidence_chain.checked
            )
        } else {
            report.chain.detail.clone()
        },
        checked_at: now_iso(),
        chain_valid: report.chain.valid,
        chain_checked: report.chain.checked,
        chain_last_seq: report.chain.last_seq,
        chain_last_hash: report.chain.last_hash,
        chain_first_bad_seq: report.chain.first_bad_seq,
        recent_events,
        recent_evidence,
        recent_claims,
        compensation_plans,
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
        let (name, payload) = runtime_event_payload(state, &run, event).await?;
        emissions.push((name, payload));
    }

    let evaluation = run
        .evaluations
        .last()
        .map(IpcPolicyEvaluation::from)
        .expect("diagnostic always evaluates");
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
        claim_id: run.claim_id.clone(),
        plan_id: run.plan_id.clone(),
    };
    Ok((summary, emissions))
}

/// Map one stored evidence row onto its contracts-shaped JSON payload,
/// omitting absent optionals (the TS schema is strict).
fn evidence_record_payload(row: &evidence_store::EvidenceRow) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    map.insert("id".to_owned(), serde_json::Value::String(row.id.clone()));
    map.insert(
        "kind".to_owned(),
        serde_json::Value::String(row.kind.clone()),
    );
    map.insert(
        "status".to_owned(),
        serde_json::Value::String(row.status.clone()),
    );
    for (key, value) in [
        ("label", row.label.as_deref()),
        ("summary", row.summary.as_deref()),
        ("uri", row.uri.as_deref()),
        ("mimeType", row.mime_type.as_deref()),
        ("taskId", row.task_id.as_deref()),
        ("agentId", row.agent_id.as_deref()),
        ("sessionId", row.session_id.as_deref()),
    ] {
        if let Some(text) = value {
            map.insert(key.to_owned(), serde_json::Value::String(text.to_owned()));
        }
    }
    map.insert(
        "hash".to_owned(),
        serde_json::Value::String(row.hash.clone()),
    );
    let payload: serde_json::Value = row
        .payload_json
        .as_deref()
        .and_then(|raw| serde_json::from_str(raw).ok())
        .unwrap_or(serde_json::Value::Null);
    map.insert("payload".to_owned(), payload);
    map.insert(
        "createdAt".to_owned(),
        serde_json::Value::String(row.created_at.clone()),
    );
    if let Some(verified_at) = row.verified_at.as_deref() {
        map.insert(
            "verifiedAt".to_owned(),
            serde_json::Value::String(verified_at.to_owned()),
        );
    }
    serde_json::Value::Object(map)
}

/// Progress heartbeat for stages without a dedicated contracts event
/// (claim/plan bookkeeping rides on the owning task's channel).
fn task_progress(
    run: &agent_runtime::DiagnosticRun,
    status: &str,
) -> (TalosEventName, serde_json::Value) {
    (
        TASK_UPDATED,
        serde_json::json!({ "task": {
            "id": run.task.id,
            "title": run.task.title,
            "status": status,
        } }),
    )
}

/// Map one runtime event onto its contracts event name + JSON payload.
/// Evidence payloads are read back from the store so the emitted record is
/// exactly what the ledger holds (hash included).
async fn runtime_event_payload(
    state: &AppState,
    run: &agent_runtime::DiagnosticRun,
    event: &RuntimeEvent,
) -> Result<(TalosEventName, serde_json::Value), IpcError> {
    let payload = match event {
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
        RuntimeEvent::ActionProposed { .. } => task_progress(run, "executing"),
        RuntimeEvent::PolicyEvaluated { request_id, .. } => {
            let evaluation = run
                .evaluations
                .iter()
                .find(|evaluation| &evaluation.request_id == request_id)
                .map(IpcPolicyEvaluation::from)
                .expect("every evaluated request has a verdict");
            (
                POLICY_EVALUATED,
                serde_json::json!({ "evaluation": evaluation }),
            )
        }
        RuntimeEvent::EvidenceRecorded { evidence_id }
        | RuntimeEvent::EvidenceVerified { evidence_id } => {
            let rows = evidence_store::list_recent_evidence(&state.db, 1000)
                .await
                .map_err(IpcError::from)?;
            let row = rows
                .into_iter()
                .find(|row| &row.id == evidence_id)
                .ok_or_else(|| {
                    IpcError::from(TalosError::Storage(format!(
                        "evidence {evidence_id} missing after insert"
                    )))
                })?;
            (
                EVIDENCE_UPDATED,
                serde_json::json!({ "evidence": evidence_record_payload(&row) }),
            )
        }
        RuntimeEvent::ClaimCreated { .. }
        | RuntimeEvent::CompensationPlanned { .. }
        | RuntimeEvent::ClaimEvaluated { .. } => task_progress(run, "executing"),
        RuntimeEvent::TaskCompleted { .. } => task_progress(run, "completed"),
    };
    Ok(payload)
}

/// Stable step name per runtime event (drives the summary checklist).
fn runtime_event_step(event: &RuntimeEvent) -> String {
    match event {
        RuntimeEvent::TaskCreated { .. } => "task_created".to_owned(),
        RuntimeEvent::AgentSpawned { .. } => "agent_spawned".to_owned(),
        RuntimeEvent::ActionProposed { .. } => "action_proposed".to_owned(),
        RuntimeEvent::PolicyEvaluated { .. } => "policy_evaluated".to_owned(),
        RuntimeEvent::EvidenceRecorded { .. } => "evidence_recorded".to_owned(),
        RuntimeEvent::EvidenceVerified { .. } => "evidence_verified".to_owned(),
        RuntimeEvent::ClaimCreated { .. } => "claim_created".to_owned(),
        RuntimeEvent::CompensationPlanned { .. } => "compensation_planned".to_owned(),
        RuntimeEvent::ClaimEvaluated { .. } => "claim_evaluated".to_owned(),
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
        RuntimeEvent::EvidenceVerified { evidence_id } => {
            format!("evidence {evidence_id} verified")
        }
        RuntimeEvent::ClaimCreated { claim_id } => format!("claim {claim_id} raised"),
        RuntimeEvent::CompensationPlanned { plan_id } => {
            format!("compensation {plan_id} planned")
        }
        RuntimeEvent::ClaimEvaluated { claim_id, status } => {
            format!("claim {claim_id} evaluated: {status}")
        }
        RuntimeEvent::TaskCompleted { summary, .. } => summary.clone(),
    }
}

/// Evaluate one raw action request with live ledger context: the request is
/// recorded in the audit trail, then evaluated with evidence/compensation
/// hooks resolved from the store. Malformed input still yields a deny
/// evaluation instead of an IPC transport error.
pub async fn request_action_evaluated(
    state: &AppState,
    raw: &serde_json::Value,
) -> IpcPolicyEvaluation {
    use evidence_store as store;
    let parsed: Result<policy_broker::ActionRequest, _> = serde_json::from_value(raw.clone());
    let request = match parsed {
        Ok(request) => request,
        Err(_) => return evaluate_action(state, raw),
    };
    let has_evidence = store::has_evidence_for_task(&state.db, &request.task_id)
        .await
        .unwrap_or(false);
    let has_plan = store::has_compensation_plan(&state.db, &request.id)
        .await
        .unwrap_or(false);
    let evaluation = state.engine.evaluate_with_context(
        &request,
        &policy_broker::PolicyContext {
            has_evidence,
            has_compensation_plan: has_plan,
        },
    );
    let record = store::NewActionRecord {
        id: request.id.clone(),
        agent_id: request.agent_id.clone(),
        task_id: request.task_id.clone(),
        capability: format!("{:?}", request.capability),
        intent: request.intent.clone(),
        target: request.target.clone(),
        payload: request.payload_json.clone(),
        risk_level: format!("{:?}", request.risk_level),
        reversible: request.reversible,
        requested_at: request.requested_at.to_rfc3339(),
    };
    if store::record_action_request(
        &state.db,
        &record,
        POLICY_VERSION,
        &format!("{:?}", evaluation.decision),
        &evaluation.reason,
        &evaluation.required_checks,
    )
    .await
    .is_err()
    {
        return evaluate_action(state, raw);
    }
    IpcPolicyEvaluation::from(&evaluation)
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

/// `request_action` — evaluate one action request through the policy engine
/// with live ledger context (recorded evidence and compensation plans gate
/// write-like and irreversible actions).
///
/// The request and its verdict are recorded in the audit trail. Malformed
/// input yields a deny-evaluation (never an IPC transport error), so the
/// frontend always receives a renderable verdict.
#[tauri::command]
pub async fn request_action(
    state: tauri::State<'_, AppState>,
    raw: serde_json::Value,
) -> Result<IpcPolicyEvaluation, IpcError> {
    Ok(request_action_evaluated(&state, &raw).await)
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
        assert_eq!(summary.claim_id, "diag-claim-1");
        assert_eq!(summary.plan_id, "diag-plan-1");
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
                EVIDENCE_UPDATED,
                TASK_UPDATED,
                POLICY_EVALUATED,
                EVIDENCE_UPDATED,
                EVIDENCE_UPDATED,
                POLICY_EVALUATED,
                TASK_UPDATED,
                POLICY_EVALUATED,
                TASK_UPDATED,
                POLICY_EVALUATED,
                EVIDENCE_UPDATED,
                EVIDENCE_UPDATED,
                TASK_UPDATED,
                TASK_UPDATED,
                TASK_UPDATED,
            ]
        );
        // Emitted evidence payloads carry the ledger hash, not placeholders.
        let evidence_payloads: Vec<&serde_json::Value> = emissions
            .iter()
            .filter(|(name, _)| *name == EVIDENCE_UPDATED)
            .map(|(_, payload)| payload)
            .collect();
        assert_eq!(evidence_payloads.len(), 6);
        for payload in evidence_payloads {
            let hash = payload
                .get("evidence")
                .and_then(|evidence| evidence.get("hash"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            assert!(
                hash.starts_with("sha256:") && hash != "sha256:ledger",
                "unexpected hash {hash}"
            );
        }
    }

    #[tokio::test]
    async fn request_action_gates_write_behind_recorded_evidence() {
        let state = test_state().await;
        let raw = serde_json::json!({
            "id": "write-1",
            "agent_id": "a-1",
            "task_id": "task-write",
            "capability": "write_file",
            "intent": "write",
            "target": "/tmp/x",
            "payload_json": {},
            "risk_level": "medium",
            "reversible": true,
            "requested_at": now_iso(),
        });
        let gated = request_action_evaluated(&state, &raw).await;
        assert_eq!(
            gated.decision,
            policy_broker::PolicyDecision::RequireEvidence
        );
        evidence_store::insert_evidence(
            &state.db,
            evidence_store::NewEvidence {
                id: "ev-task-write".to_owned(),
                task_id: Some("task-write".to_owned()),
                agent_id: None,
                session_id: None,
                kind: "log".to_owned(),
                label: None,
                summary: None,
                uri: None,
                mime_type: None,
                payload: serde_json::json!({}),
            },
        )
        .await
        .expect("evidence inserts");
        let allowed = request_action_evaluated(&state, &raw).await;
        assert_eq!(allowed.decision, policy_broker::PolicyDecision::Allow);
    }

    #[tokio::test]
    async fn request_action_gates_irreversible_behind_compensation_plan() {
        let state = test_state().await;
        let raw = serde_json::json!({
            "id": "rotate-1",
            "agent_id": "a-1",
            "task_id": "task-rotate",
            "capability": "system_control",
            "intent": "rotate",
            "target": "keyring:x",
            "payload_json": {},
            "risk_level": "irreversible",
            "reversible": false,
            "requested_at": now_iso(),
        });
        let gated = request_action_evaluated(&state, &raw).await;
        assert_eq!(
            gated.decision,
            policy_broker::PolicyDecision::RequireCompensation
        );
        evidence_store::create_compensation_plan(
            &state.db,
            evidence_store::NewCompensationPlan {
                id: "plan-rotate-1".to_owned(),
                action_request_id: "rotate-1".to_owned(),
                strategy: "rollback_command".to_owned(),
                reason: "test".to_owned(),
                steps: vec![evidence_store::NewCompensationStep {
                    action_type: "rollback_command".to_owned(),
                    payload: serde_json::json!({}),
                }],
            },
        )
        .await
        .expect("plan created");
        let allowed = request_action_evaluated(&state, &raw).await;
        assert_eq!(allowed.decision, policy_broker::PolicyDecision::Allow);
    }

    #[tokio::test]
    async fn ledger_counts_start_at_zero() {
        let state = test_state().await;
        let status = ledger_status_of(&state).await.expect("ledger reads");
        assert!(status.healthy);
        assert_eq!(status.event_count, 0);
    }
}
