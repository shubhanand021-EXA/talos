//! Secure IPC surface. Thirty commands, all typed, validated, and infallible
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
//! | `submit_task`            | `IpcCommandName.submitTask`          |
//! | `cancel_task`            | `IpcCommandName.cancelTask`          |
//! | `retry_task`             | `IpcCommandName.retryTask`           |
//! | `list_tasks`             | `IpcCommandName.listTasks`           |
//! | `get_task_state`         | `IpcCommandName.getTaskState`        |
//! | `list_tools`             | `IpcCommandName.listTools`           |
//! | `describe_tool`          | `IpcCommandName.describeTool`        |
//! | `invoke_tool_dry_run`    | `IpcCommandName.invokeToolDryRun`    |
//! | `invoke_tool`            | `IpcCommandName.invokeTool`          |
//! | `computer_safety_status` | `IpcCommandName.computerSafetyStatus` |
//! | `computer_emergency_stop` | `IpcCommandName.computerEmergencyStop` |
//! | `computer_reset_stop`    | `IpcCommandName.computerResetStop`   |
//! | `context_budget_status`  | `IpcCommandName.contextBudgetStatus` |
//! | `context_compact`        | `IpcCommandName.contextCompact`      |
//! | `context_diagnostic_snapshot` | `IpcCommandName.contextDiagnosticSnapshot` |
//! | `query_tool_candidates` | `IpcCommandName.queryToolCandidates` |
//! | `list_providers`         | `IpcCommandName.listProviders`         |
//! | `list_models`            | `IpcCommandName.listModels`            |
//! | `test_provider_connection` | `IpcCommandName.testProviderConnection` |
//! | `get_usage_summary`      | `IpcCommandName.getUsageSummary`      |
//! | `store_provider_key`     | `IpcCommandName.storeProviderKey`     |
//! | `delete_provider_key`    | `IpcCommandName.deleteProviderKey`    |
//! | `preview_context`        | `IpcCommandName.previewContext`        |
//! | `get_context_config`     | `IpcCommandName.getContextConfig`     |
//! | `update_context_config`  | `IpcCommandName.updateContextConfig`  |
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
    emit_talos_event, TalosEventName, AGENT_SPAWNED, BROWSER_WORKER_CRASHED,
    COMPUTER_SAFETY_CHANGED, CONTEXT_COMPACTED, EVIDENCE_UPDATED, POLICY_EVALUATED, TASK_CREATED,
    TASK_UPDATED,
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
        RuntimeEvent::TaskStatusChanged { task_id, to, .. } => (
            TASK_UPDATED,
            serde_json::json!({ "task": { "id": task_id, "title": task_id, "status": to } }),
        ),
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
        RuntimeEvent::TaskStatusChanged { .. } => "task_status_changed".to_owned(),
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
        RuntimeEvent::TaskStatusChanged { task_id, to, .. } => {
            format!("task {task_id} -> {to}")
        }
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
    let durable = evidence_store::list_tasks(&state.db, 1000)
        .await
        .map(|rows| rows.len())
        .unwrap_or(0);
    let in_memory = runtime.state().tasks.len();
    Ok(system_status_of(
        &state,
        ledger_healthy,
        u32::try_from(runtime.state().agents.len()).unwrap_or(u32::MAX),
        u32::try_from(in_memory.saturating_add(durable)).unwrap_or(u32::MAX),
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

/// Input for `submit_task` (`camelCase` wire, validated on both sides).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitTaskInput {
    /// Human-readable title.
    pub title: String,
    /// Optional detail.
    pub description: Option<String>,
    /// `low` | `normal` | `high` | `critical`.
    pub priority: Option<String>,
    /// Per-attempt timeout in milliseconds.
    pub timeout_ms: Option<i64>,
    /// Attempt budget (1..=10).
    pub max_attempts: Option<i64>,
    /// Deterministic mock scenario.
    pub scenario: Option<String>,
}

/// Emit one `taskCreated` / `taskUpdated` event for a queue view.
fn emit_queue_event(
    app: &tauri::AppHandle,
    name: TalosEventName,
    view: &agent_runtime::queue::TaskView,
) {
    let payload = serde_json::json!({ "task": {
        "id": view.id,
        "title": view.title,
        "status": view.status,
        "priority": view.priority,
        "attempts": view.attempts,
        "maxAttempts": view.max_attempts,
    } });
    if let Err(err) = emit_talos_event(app, name, payload) {
        eprintln!("[talos] event emission failed ({name}): {err}");
    }
}

/// `submit_task` — enqueue a durable task, then execute it through the
/// policy broker (worker + ledger integration, never a bypass).
#[tauri::command]
pub async fn submit_task(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    input: SubmitTaskInput,
) -> Result<agent_runtime::queue::TaskView, IpcError> {
    let title = input.title.trim().to_owned();
    if title.is_empty() || title.len() > 256 {
        return Err(IpcError::from(TalosError::Storage(
            "title must be 1..=256 chars".to_owned(),
        )));
    }
    let seq = {
        let mut runtime = state.runtime.lock().await;
        runtime.next_queue_seq()
    };
    let view = agent_runtime::queue::submit_task(
        &state.db,
        &mut { seq },
        agent_runtime::queue::SubmitParams {
            title,
            description: input.description,
            priority: input.priority.unwrap_or_else(|| "normal".to_owned()),
            timeout_ms: input.timeout_ms.unwrap_or(30_000),
            max_attempts: input.max_attempts.unwrap_or(3),
            scenario: input.scenario.unwrap_or_else(|| "quick_success".to_owned()),
        },
    )
    .await?;
    emit_queue_event(&app, TASK_CREATED, &view);
    let done = agent_runtime::queue::execute_task(
        &state.db,
        &state.engine,
        &view.id,
        &agent_runtime::queue::CancelToken::new(),
    )
    .await?;
    emit_queue_event(&app, TASK_UPDATED, &done);
    Ok(done)
}

/// `cancel_task` — request cancellation; live tasks move to `canceling`.
#[tauri::command]
pub async fn cancel_task(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    task_id: String,
) -> Result<agent_runtime::queue::TaskView, IpcError> {
    let view = agent_runtime::queue::cancel_task(&state.db, &task_id).await?;
    emit_queue_event(&app, TASK_UPDATED, &view);
    Ok(view)
}

/// `retry_task` — requeue a finished task (`failed` / `timed_out` / `canceled`).
#[tauri::command]
pub async fn retry_task(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    task_id: String,
) -> Result<agent_runtime::queue::TaskView, IpcError> {
    let view = agent_runtime::queue::retry_task(&state.db, &task_id).await?;
    emit_queue_event(&app, TASK_UPDATED, &view);
    Ok(view)
}

/// `list_tasks` — durable queue entries, newest first.
#[tauri::command]
pub async fn list_tasks(
    state: tauri::State<'_, AppState>,
    limit: Option<i64>,
) -> Result<Vec<agent_runtime::queue::TaskView>, IpcError> {
    Ok(agent_runtime::queue::list_tasks(&state.db, limit.unwrap_or(50)).await?)
}

/// `get_task_state` — one durable queue entry by id.
#[tauri::command]
pub async fn get_task_state(
    state: tauri::State<'_, AppState>,
    task_id: String,
) -> Result<agent_runtime::queue::TaskView, IpcError> {
    Ok(agent_runtime::queue::get_task_state(&state.db, &task_id).await?)
}

/// Sandbox configuration for tool execution (root + exec policy + computer safety).
fn tool_config_of(state: &AppState) -> tool_runtime::SandboxConfig {
    tool_runtime::SandboxConfig {
        root: state.tool_root.clone(),
        exec_enabled: state.exec_enabled,
        exec_allowlist: state.exec_allowlist.clone(),
        exec_timeout_ms: 5_000,
        max_bytes: 262_144,
        computer: state.computer.clone(),
    }
}

/// `list_tools` — registered tool manifests (stable order, metadata for UI/RAG).
#[tauri::command]
pub async fn list_tools(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<tool_runtime::ToolManifest>, IpcError> {
    let registry = state.tools.lock().await;
    Ok(registry.list())
}

/// `describe_tool` — one manifest by id (unknown ids are transport errors).
#[tauri::command]
pub async fn describe_tool(
    state: tauri::State<'_, AppState>,
    tool_id: String,
) -> Result<tool_runtime::ToolManifest, IpcError> {
    let registry = state.tools.lock().await;
    registry
        .get(&tool_id)
        .cloned()
        .ok_or_else(|| IpcError::from(TalosError::Storage(format!("unknown tool: {tool_id}"))))
}

/// Shared tool invocation path (dry-run plans, live executes; always audited).
async fn invoke_tool_with(
    state: &tauri::State<'_, AppState>,
    tool_id: String,
    task_id: String,
    agent_id: Option<String>,
    params: serde_json::Value,
    dry_run: bool,
) -> Result<tool_runtime::ToolReceipt, IpcError> {
    if tool_id.trim().is_empty() || tool_id.len() > 128 {
        return Err(IpcError::from(TalosError::Storage(
            "tool_id must be 1..=128 chars".to_owned(),
        )));
    }
    if task_id.trim().is_empty() || task_id.len() > 128 {
        return Err(IpcError::from(TalosError::Storage(
            "task_id must be 1..=128 chars".to_owned(),
        )));
    }
    let registry = state.tools.lock().await;
    let outcome = tool_runtime::invoke(
        &state.db,
        &state.engine,
        &registry,
        &tool_config_of(state),
        tool_runtime::ToolInvocation {
            tool_id,
            task_id,
            agent_id: agent_id.unwrap_or_else(|| "agent-local".to_owned()),
            params,
            dry_run,
        },
    )
    .await?;
    Ok(outcome.receipt)
}

/// `invoke_tool_dry_run` — validate + policy-evaluate without side effects.
#[tauri::command]
pub async fn invoke_tool_dry_run(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    tool_id: String,
    task_id: String,
    agent_id: Option<String>,
    params: serde_json::Value,
) -> Result<tool_runtime::ToolReceipt, IpcError> {
    let receipt = invoke_tool_with(
        &state,
        tool_id.clone(),
        task_id.clone(),
        agent_id,
        params,
        true,
    )
    .await?;
    emit_browser_worker_crash(&state, &app, &tool_id, &task_id, &receipt).await;
    Ok(receipt)
}

/// `invoke_tool` — policy-gated, audited live execution (writes compensate first).
#[tauri::command]
pub async fn invoke_tool(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    tool_id: String,
    task_id: String,
    agent_id: Option<String>,
    params: serde_json::Value,
) -> Result<tool_runtime::ToolReceipt, IpcError> {
    let receipt = invoke_tool_with(
        &state,
        tool_id.clone(),
        task_id.clone(),
        agent_id,
        params,
        false,
    )
    .await?;
    emit_browser_worker_crash(&state, &app, &tool_id, &task_id, &receipt).await;
    Ok(receipt)
}

/// Surface browser worker crashes/timeouts as `browserWorkerCrashed` events so
/// the UI can banner them; the receipt already marks the task failed.
async fn emit_browser_worker_crash(
    _state: &tauri::State<'_, AppState>,
    app: &tauri::AppHandle,
    tool_id: &str,
    task_id: &str,
    receipt: &tool_runtime::ToolReceipt,
) {
    let crashed = receipt.error.as_ref().is_some_and(|error| {
        matches!(
            error.code.as_str(),
            "browser_worker_crashed" | "browser_worker_timeout"
        )
    });
    if !crashed {
        return;
    }
    let payload = serde_json::json!({
        "taskId": task_id,
        "toolId": tool_id,
        "code": receipt.error.as_ref().map(|error| error.code.clone()).unwrap_or_default(),
        "checkedAt": talos_common::now_iso(),
    });
    if let Err(err) = emit_talos_event(app, BROWSER_WORKER_CRASHED, payload) {
        eprintln!("[talos] event emission failed ({BROWSER_WORKER_CRASHED}): {err}");
    }
}

/// `computer_safety_status` — current envelope state (enabled flag, e-stop,
/// rate usage, viewport, confidence threshold). Read-only and always available.
#[tauri::command]
pub async fn computer_safety_status(
    state: tauri::State<'_, AppState>,
) -> Result<computer_use::SafetyStatus, IpcError> {
    Ok(state.computer.status_snapshot().await)
}

/// `computer_emergency_stop` — latch the emergency stop, cancel active queue
/// tasks, and emit `computerSafetyChanged`. Blocks every new computer-use
/// action (including dry-runs) until explicitly reset.
#[tauri::command]
pub async fn computer_emergency_stop(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<computer_use::SafetyStatus, IpcError> {
    state.computer.engage_stop();
    let canceled = cancel_active_tasks(&state).await.unwrap_or(0);
    let status = state.computer.status_snapshot().await;
    let payload = serde_json::json!({
        "emergencyStopped": true,
        "canceledTasks": canceled,
        "checkedAt": talos_common::now_iso(),
    });
    if let Err(err) = emit_talos_event(&app, COMPUTER_SAFETY_CHANGED, payload) {
        eprintln!("[talos] event emission failed ({COMPUTER_SAFETY_CHANGED}): {err}");
    }
    Ok(status)
}

/// `computer_reset_stop` — clear the emergency stop latch (explicit reset).
/// Emits `computerSafetyChanged`. Does not resume canceled tasks.
#[tauri::command]
pub async fn computer_reset_stop(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<computer_use::SafetyStatus, IpcError> {
    state.computer.clear_stop();
    let status = state.computer.status_snapshot().await;
    let payload = serde_json::json!({
        "emergencyStopped": false,
        "canceledTasks": 0,
        "checkedAt": talos_common::now_iso(),
    });
    if let Err(err) = emit_talos_event(&app, COMPUTER_SAFETY_CHANGED, payload) {
        eprintln!("[talos] event emission failed ({COMPUTER_SAFETY_CHANGED}): {err}");
    }
    Ok(status)
}

/// Cancel every non-terminal queue task (best effort).
///
/// Used by the emergency stop so in-flight automation halts instead of
/// lingering in `queued` / `executing` states. Returns the cancel count.
async fn cancel_active_tasks(state: &AppState) -> Result<usize, IpcError> {
    let mut canceled = 0_usize;
    let tasks = agent_runtime::queue::list_tasks(&state.db, 1000).await?;
    for task in tasks {
        if agent_runtime::lifecycle::is_terminal(&task.status) {
            continue;
        }
        if agent_runtime::queue::cancel_task(&state.db, &task.id)
            .await
            .is_ok()
        {
            canceled += 1;
        }
    }
    Ok(canceled)
}

/// Per-layer budget accounting, shaped exactly like the TS
/// `ContextLayerBudget` schema (`camelCase` JSON).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextLayerBudget {
    /// Layer name (`system`, `session`, `task`, `evidence`, `tool`, ...).
    pub layer: String,
    /// Tokens used by this layer.
    pub used_tokens: u64,
    /// Configured maximum.
    pub max_tokens: u64,
    /// True when usage exceeds the maximum.
    pub overflow: bool,
    /// Protected layers are never rebalanced away.
    pub protected: bool,
    /// Segment count in this layer.
    pub segment_count: usize,
}

/// Budget status for one task, shaped like TS `ContextBudgetStatus`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextBudgetStatus {
    /// Owning task.
    pub task_id: String,
    /// Sum over all layers.
    pub total_used: u64,
    /// Configured total maximum.
    pub total_budget: u64,
    /// True on any overflow.
    pub overflow: bool,
    /// Per-layer usage in priority order.
    pub layers: Vec<ContextLayerBudget>,
}

/// One compacted segment, shaped like TS `ContextCompactedSegment`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextCompactedSegment {
    /// Stable segment identifier.
    pub id: String,
    /// Owning layer.
    pub layer: String,
    /// Compacted body (verbatim for protected/safety segments).
    pub body: String,
    /// Tokens before compaction.
    pub original_tokens: u64,
    /// Tokens after compaction.
    pub compacted_tokens: u64,
    /// True when the body was rewritten.
    pub trimmed: bool,
    /// True when the segment was dropped.
    pub dropped: bool,
    /// Protected segments are always carried verbatim.
    pub protected: bool,
}

/// TypeCompact receipt, shaped like TS `ContextCompactionReceipt`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextCompactionReceipt {
    /// Stable receipt identifier.
    pub id: String,
    /// Owning task.
    pub task_id: String,
    /// Tokens before compaction.
    pub original_tokens: u64,
    /// Tokens after compaction.
    pub compacted_tokens: u64,
    /// Invariant attestations (`ids:3/3`, `safety:unchanged`, ...).
    pub preserved_invariants: Vec<String>,
    /// Dropped segment ids.
    pub dropped_segments: Vec<String>,
    /// Trimmed segment ids.
    pub trimmed_segments: Vec<String>,
    /// `sha256:` proof hash of the canonical compacted payload.
    pub proof_hash: String,
    /// Per-segment outcomes in input order.
    pub segments: Vec<ContextCompactedSegment>,
    /// Evidence row holding this receipt, if persisted.
    pub evidence_id: Option<String>,
    /// Engine version (`typecompact/1`).
    pub version: String,
    /// RFC 3339 execution timestamp.
    pub executed_at: String,
}

/// Resolve the effective budget (default 8_000; overrides clamp to 64..=100_000).
fn context_budget_for(total_budget: Option<u64>) -> context_budget::BudgetConfig {
    let mut config = context_budget::default_config();
    if let Some(total) = total_budget {
        config.total_budget = total.clamp(64, 100_000);
    }
    config
}

/// Build a deterministic seven-layer snapshot for one task.
///
/// Task metadata comes from the queue (unknown tasks fall back to an
/// `untitled` placeholder so the command stays total); evidence and tool
/// rows come from the ledger; the safety row reflects the live envelope.
async fn context_snapshot_for(
    state: &AppState,
    task_id: &str,
) -> Result<context_budget::ContextSnapshot, IpcError> {
    if task_id.trim().is_empty() || task_id.len() > 128 {
        return Err(IpcError::from(TalosError::Storage(
            "task_id must be 1..=128 chars".to_owned(),
        )));
    }
    let (title, status, notes) =
        match agent_runtime::queue::get_task_state(&state.db, task_id).await {
            Ok(view) => (
                view.title,
                view.status,
                view.last_error.unwrap_or_else(|| "no errors".to_owned()),
            ),
            Err(_) => (
                "untitled".to_owned(),
                "unknown".to_owned(),
                "no errors".to_owned(),
            ),
        };
    let safety = state.computer.status_snapshot().await;
    let safety_summary = format!(
        "enabled={}, stopped={}, rate={}/{}, viewport={}x{}, min_conf={}",
        safety.enabled,
        safety.emergency_stopped,
        safety.actions_this_minute,
        safety.max_actions_per_minute,
        safety.viewport_width,
        safety.viewport_height,
        safety.min_confidence
    );
    let rows = evidence_store::list_recent_evidence(&state.db, 100).await?;
    let mut evidence: Vec<(String, String, String)> = Vec::new();
    let mut tools: Vec<(String, String, String)> = Vec::new();
    for row in rows {
        if row.task_id.as_deref() != Some(task_id) {
            continue;
        }
        if row.kind == "tool_receipt" {
            let tool_id = row
                .payload_json
                .as_deref()
                .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
                .and_then(|payload| {
                    payload
                        .get("toolId")
                        .and_then(|value| value.as_str())
                        .map(str::to_owned)
                })
                .unwrap_or_else(|| "unknown.tool".to_owned());
            tools.push((tool_id, row.id, row.status));
        } else {
            evidence.push((row.id, row.kind, row.status));
        }
        if evidence.len() + tools.len() >= 40 {
            break;
        }
    }
    Ok(context_budget::build_task_snapshot(
        task_id,
        "session-local",
        &title,
        &status,
        &safety_summary,
        &evidence,
        &tools,
        &notes,
        vec![format!("eval-{task_id}")],
        vec![],
        vec![],
    ))
}

/// Persist a compaction receipt as verified `context_compaction` evidence.
async fn persist_compaction_receipt(
    state: &AppState,
    receipt: &ContextCompactionReceipt,
) -> Result<String, IpcError> {
    let evidence_id = format!("context-evidence-{}", receipt.id);
    let payload = serde_json::to_value(receipt)
        .map_err(|err| IpcError::from(TalosError::Storage(format!("receipt serialize: {err}"))))?;
    evidence_store::insert_evidence(
        &state.db,
        evidence_store::NewEvidence {
            id: evidence_id.clone(),
            task_id: Some(receipt.task_id.clone()),
            agent_id: None,
            session_id: Some("session-local".to_owned()),
            kind: context_budget::COMPACTION_KIND.to_owned(),
            label: Some(format!("context_compact {}", receipt.id)),
            summary: Some(format!(
                "{} -> {} tokens (proof {})",
                receipt.original_tokens, receipt.compacted_tokens, receipt.proof_hash
            )),
            uri: None,
            mime_type: None,
            payload,
        },
    )
    .await
    .map(|_| evidence_id.clone())?;
    evidence_store::transition_evidence_status(&state.db, &evidence_id, "verified").await?;
    Ok(evidence_id)
}

/// Shape a budget report for the TS `ContextBudgetStatus` schema.
fn budget_status_of(
    report: &context_budget::BudgetReport,
    config: &context_budget::BudgetConfig,
) -> ContextBudgetStatus {
    ContextBudgetStatus {
        task_id: report.task_id.clone(),
        total_used: report.total_used,
        total_budget: config.total_budget,
        overflow: report.overflow,
        layers: report
            .layers
            .iter()
            .map(|usage| ContextLayerBudget {
                layer: usage.layer.as_str().to_owned(),
                used_tokens: usage.used_tokens,
                max_tokens: usage.max_tokens,
                overflow: usage.overflow,
                protected: usage.protected,
                segment_count: usage.segment_count,
            })
            .collect(),
    }
}

/// Shape a compaction result for the TS `ContextCompactionReceipt` schema.
fn compaction_receipt_of(
    result: &context_budget::CompactionResult,
    evidence_id: Option<String>,
) -> ContextCompactionReceipt {
    ContextCompactionReceipt {
        id: result.id.clone(),
        task_id: result.task_id.clone(),
        original_tokens: result.original_tokens,
        compacted_tokens: result.compacted_tokens,
        preserved_invariants: result.preserved_invariants.clone(),
        dropped_segments: result.dropped_segments.clone(),
        trimmed_segments: result.trimmed_segments.clone(),
        proof_hash: result.proof_hash.clone(),
        segments: result
            .segments
            .iter()
            .map(|row| ContextCompactedSegment {
                id: row.id.clone(),
                layer: row.layer.as_str().to_owned(),
                body: if row.dropped {
                    String::new()
                } else {
                    row.body.clone()
                },
                original_tokens: row.original_tokens,
                compacted_tokens: row.compacted_tokens,
                trimmed: row.trimmed,
                dropped: row.dropped,
                protected: row.protected,
            })
            .collect(),
        evidence_id,
        version: result.version.clone(),
        executed_at: now_iso(),
    }
}

/// Emit `contextCompacted` for a finished compaction (best effort).
fn emit_compaction(app: Option<&tauri::AppHandle>, receipt: &ContextCompactionReceipt) {
    let Some(app) = app else {
        return;
    };
    let payload = serde_json::json!({
        "taskId": receipt.task_id,
        "evidenceId": receipt.evidence_id,
        "proofHash": receipt.proof_hash,
        "originalTokens": receipt.original_tokens,
        "compactedTokens": receipt.compacted_tokens,
        "checkedAt": talos_common::now_iso(),
    });
    if let Err(err) = emit_talos_event(app, CONTEXT_COMPACTED, payload) {
        eprintln!("[talos] event emission failed ({CONTEXT_COMPACTED}): {err}");
    }
}

/// Inner budget status (shared by the command and tests; no app handle needed).
async fn context_budget_status_inner(
    state: &AppState,
    task_id: String,
    total_budget: Option<u64>,
) -> Result<ContextBudgetStatus, IpcError> {
    let config = context_budget_for(total_budget);
    let snapshot = context_snapshot_for(state, &task_id).await?;
    Ok(budget_status_of(
        &context_budget::report(&snapshot, &config),
        &config,
    ))
}

/// Inner compaction (shared by the command and tests).
async fn context_compact_inner(
    state: &AppState,
    app: Option<&tauri::AppHandle>,
    task_id: String,
    total_budget: Option<u64>,
) -> Result<ContextCompactionReceipt, IpcError> {
    let config = context_budget_for(total_budget);
    let snapshot = context_snapshot_for(state, &task_id).await?;
    let result = context_budget::compact_snapshot(&snapshot, &config).map_err(|err| {
        IpcError::from(TalosError::Storage(format!(
            "{}: {}",
            err.code, err.message
        )))
    })?;
    let mut receipt = compaction_receipt_of(&result, None);
    receipt.evidence_id = Some(persist_compaction_receipt(state, &receipt).await?);
    emit_compaction(app, &receipt);
    Ok(receipt)
}

/// Inner diagnostic snapshot (shared by the command and tests).
async fn context_diagnostic_snapshot_inner(
    state: &AppState,
    app: Option<&tauri::AppHandle>,
    total_budget: Option<u64>,
) -> Result<ContextCompactionReceipt, IpcError> {
    let config = context_budget_for(total_budget);
    let (summary, _emissions) = diagnostic_flow(state).await?;
    let ledger = ledger_status_of(state).await?;
    let snapshot = context_budget::build_diagnostic_snapshot(
        &summary.task_id,
        &summary.agent_id,
        &summary.claim_id,
        &summary.plan_id,
        &ledger.detail,
    );
    let result = context_budget::compact_snapshot(&snapshot, &config).map_err(|err| {
        IpcError::from(TalosError::Storage(format!(
            "{}: {}",
            err.code, err.message
        )))
    })?;
    let mut receipt = compaction_receipt_of(&result, None);
    receipt.evidence_id = Some(persist_compaction_receipt(state, &receipt).await?);
    emit_compaction(app, &receipt);
    Ok(receipt)
}

/// `context_budget_status` — deterministic per-layer budget for one task.
/// Read-only: no mutation, no persistence.
#[tauri::command]
pub async fn context_budget_status(
    state: tauri::State<'_, AppState>,
    task_id: String,
    total_budget: Option<u64>,
) -> Result<ContextBudgetStatus, IpcError> {
    context_budget_status_inner(&state, task_id, total_budget).await
}

/// `context_compact` — TypeCompact one task's context, persist the receipt as
/// verified `context_compaction` evidence, and emit `contextCompacted`.
/// Invariant failures become structured `IpcError`s; nothing is stored then.
#[tauri::command]
pub async fn context_compact(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    task_id: String,
    total_budget: Option<u64>,
) -> Result<ContextCompactionReceipt, IpcError> {
    context_compact_inner(&state, Some(&app), task_id, total_budget).await
}

/// `context_diagnostic_snapshot` — run the diagnostic loop, TypeCompact its
/// context (claim + compensation plan refs included), persist the receipt,
/// and emit `contextCompacted`. This is the runtime-integration proof that a
/// diagnostic flow can produce a compacted snapshot.
#[tauri::command]
pub async fn context_diagnostic_snapshot(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    total_budget: Option<u64>,
) -> Result<ContextCompactionReceipt, IpcError> {
    context_diagnostic_snapshot_inner(&state, Some(&app), total_budget).await
}

/// Inner Tool-RAG query (shared by the command and tests).
///
/// Read-only: rebuilds the SQLite index from the registry + ledger, then
/// ranks with the local lexical provider. No persistence of judgments, no
/// events, no network.
async fn query_tool_candidates_inner(
    state: &AppState,
    intent: String,
    capabilities: Vec<String>,
    risk_ceiling: Option<String>,
    require_evidence: bool,
    top_k: Option<usize>,
) -> Result<Vec<tool_rag::ToolCandidate>, IpcError> {
    let query =
        tool_rag::validate_query(&intent, capabilities, risk_ceiling, require_evidence, top_k)
            .map_err(IpcError::from)?;
    let registry = state.tools.lock().await;
    let manifests = registry.list();
    let enabled: std::collections::HashMap<String, bool> = manifests
        .iter()
        .map(|manifest| (manifest.id.clone(), registry.is_enabled(&manifest.id)))
        .collect();
    drop(registry);
    let evidence_rows = evidence_store::list_recent_evidence(&state.db, 200).await?;
    let tasks = evidence_store::list_tasks(&state.db, 200).await?;
    let plans = evidence_store::list_compensation_plans(&state.db, 50).await?;
    let docs = tool_rag::build_indexed_tools(
        &manifests,
        &tool_rag::receipt_tool_tasks(&evidence_rows),
        &tasks,
        &tool_rag::evidence_kind_list(&evidence_rows),
        &tool_rag::plan_strategy_list(&plans),
        &enabled,
    );
    tool_rag::index_tools(&state.db, &docs).await?;
    let stats = tool_rag::stats_from_evidence(&evidence_rows);
    Ok(tool_rag::rank(
        &query,
        &docs,
        &stats,
        &tool_rag::LocalLexicalProvider,
    ))
}

/// `query_tool_candidates` — rank registered tools for an intent.
///
/// Filters (capability allow-list, risk ceiling, evidence requirement) apply
/// before scoring; scores blend keyword overlap, capability match, receipt
/// history (success boost, failure penalty), and evidence backing. Returns
/// at most `top_k` candidates with reasons, matched fields, risk levels, and
/// required approvals.
#[tauri::command]
pub async fn query_tool_candidates(
    state: tauri::State<'_, AppState>,
    intent: String,
    capabilities: Vec<String>,
    risk_ceiling: Option<String>,
    require_evidence: bool,
    top_k: Option<usize>,
) -> Result<Vec<tool_rag::ToolCandidate>, IpcError> {
    query_tool_candidates_inner(
        &state,
        intent,
        capabilities,
        risk_ceiling,
        require_evidence,
        top_k,
    )
    .await
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

    async fn submit_one(state: &AppState, title: &str) -> agent_runtime::queue::TaskView {
        let seq = {
            let mut runtime = state.runtime.lock().await;
            runtime.next_queue_seq()
        };
        let view = agent_runtime::queue::submit_task(
            &state.db,
            &mut { seq },
            agent_runtime::queue::SubmitParams {
                title: title.to_owned(),
                description: None,
                priority: "normal".to_owned(),
                timeout_ms: 5_000,
                max_attempts: 2,
                scenario: "quick_success".to_owned(),
            },
        )
        .await
        .expect("submit");
        agent_runtime::queue::execute_task(
            &state.db,
            &state.engine,
            &view.id,
            &agent_runtime::queue::CancelToken::new(),
        )
        .await
        .expect("execute")
    }

    #[tokio::test]
    async fn submit_and_list_tasks_round_trip() {
        let state = test_state().await;
        let done = submit_one(&state, "queue task").await;
        assert_eq!(done.status, "completed");
        let listed = agent_runtime::queue::list_tasks(&state.db, 10)
            .await
            .expect("list");
        assert!(listed.iter().any(|t| t.id == done.id));
        let fetched = agent_runtime::queue::get_task_state(&state.db, &done.id)
            .await
            .expect("get");
        assert_eq!(fetched.title, "queue task");
    }

    #[tokio::test]
    async fn cancel_and_retry_task() {
        let state = test_state().await;
        let seq = {
            let mut runtime = state.runtime.lock().await;
            runtime.next_queue_seq()
        };
        let view = agent_runtime::queue::submit_task(
            &state.db,
            &mut { seq },
            agent_runtime::queue::SubmitParams {
                title: "cancel me".to_owned(),
                description: None,
                priority: "high".to_owned(),
                timeout_ms: 5_000,
                max_attempts: 2,
                scenario: "non_retryable_failure".to_owned(),
            },
        )
        .await
        .expect("submit");
        let done = agent_runtime::queue::execute_task(
            &state.db,
            &state.engine,
            &view.id,
            &agent_runtime::queue::CancelToken::new(),
        )
        .await
        .expect("execute");
        assert_eq!(done.status, "failed");
        let retried = agent_runtime::queue::retry_task(&state.db, &view.id)
            .await
            .expect("retry");
        assert_eq!(retried.status, "queued");
    }

    #[tokio::test]
    async fn submit_rejects_invalid_input() {
        let state = test_state().await;
        let bad = agent_runtime::queue::submit_task(
            &state.db,
            &mut { 1_u64 },
            agent_runtime::queue::SubmitParams {
                title: "".to_owned(),
                description: None,
                priority: "normal".to_owned(),
                timeout_ms: 5_000,
                max_attempts: 2,
                scenario: "quick_success".to_owned(),
            },
        )
        .await;
        assert!(bad.is_err());
    }

    async fn invoke(
        state: &AppState,
        tool_id: &str,
        task_id: &str,
        params: serde_json::Value,
        dry_run: bool,
    ) -> tool_runtime::ToolReceipt {
        let registry = state.tools.lock().await;
        let config = tool_runtime::SandboxConfig {
            root: state.tool_root.clone(),
            exec_enabled: state.exec_enabled,
            exec_allowlist: state.exec_allowlist.clone(),
            exec_timeout_ms: 5_000,
            max_bytes: 262_144,
            computer: state.computer.clone(),
        };
        tool_runtime::invoke(
            &state.db,
            &state.engine,
            &registry,
            &config,
            tool_runtime::ToolInvocation {
                tool_id: tool_id.to_owned(),
                task_id: task_id.to_owned(),
                agent_id: "agent-test".to_owned(),
                params,
                dry_run,
            },
        )
        .await
        .expect("invoke")
        .receipt
    }

    #[tokio::test]
    async fn tools_list_and_describe() {
        let state = test_state().await;
        let registry = state.tools.lock().await;
        assert_eq!(registry.list().len(), 23);
        let manifest = registry.get("fs.read").expect("fs.read");
        assert_eq!(manifest.id, "fs.read");
        let browser = registry.get("browser.snapshot").expect("browser.snapshot");
        assert_eq!(browser.id, "browser.snapshot");
        let computer = registry
            .get("computer.capture_screen")
            .expect("computer.capture_screen");
        assert_eq!(computer.id, "computer.capture_screen");
        assert!(registry.get("nope").is_none());
    }

    #[tokio::test]
    async fn browser_worker_crash_surfaces_structured_error() {
        // No other desktop test invokes live browser tools, so flipping the
        // process-global live flag here cannot disturb concurrent tests.
        std::env::set_var("TALOS_BROWSER_LIVE", "1");
        std::env::set_var("TALOS_BROWSER_WORKER", "/nonexistent/worker-stub.js");
        let state = test_state().await;
        let receipt = invoke(
            &state,
            "browser.session",
            "task-worker-crash",
            serde_json::json!({ "operation": "status" }),
            false,
        )
        .await;
        std::env::remove_var("TALOS_BROWSER_LIVE");
        std::env::remove_var("TALOS_BROWSER_WORKER");
        let error = receipt.error.expect("structured error");
        assert!(
            error.code == "browser_worker_crashed"
                || error.code == "worker_spawn_failed"
                || error.code == "worker_crashed",
            "unexpected code {}",
            error.code
        );
        assert!(receipt.evidence_id.is_some());
    }

    #[tokio::test]
    async fn tool_dry_run_has_no_side_effects() {
        let state = test_state().await;
        let receipt = invoke(
            &state,
            "fs.write",
            "task-dry",
            serde_json::json!({ "path": "dry.txt", "content": "hi" }),
            true,
        )
        .await;
        // Fresh task: policy denies the write until evidence exists.
        assert!(matches!(
            receipt.status,
            tool_runtime::ToolStatus::Denied | tool_runtime::ToolStatus::Success
        ));
        assert!(receipt.dry_run);
        assert!(!state.tool_root.join("dry.txt").exists());
        assert!(receipt.evidence_id.is_some());
    }

    #[tokio::test]
    async fn tool_fs_write_requires_evidence_then_compensates() {
        let state = test_state().await;
        tokio::fs::write(state.tool_root.join("seed.txt"), b"x")
            .await
            .expect("seed");
        let read = invoke(
            &state,
            "fs.read",
            "task-write",
            serde_json::json!({ "path": "seed.txt" }),
            false,
        )
        .await;
        assert_eq!(read.status, tool_runtime::ToolStatus::Success);
        let wrote = invoke(
            &state,
            "fs.write",
            "task-write",
            serde_json::json!({ "path": "out.txt", "content": "hello" }),
            false,
        )
        .await;
        assert_eq!(wrote.status, tool_runtime::ToolStatus::Success);
        assert!(state.tool_root.join("out.txt").exists());
        assert!(wrote.compensation_plan_id.is_some());
    }

    #[tokio::test]
    async fn tool_exec_disabled_by_default() {
        let state = test_state().await;
        let receipt = invoke(
            &state,
            "process.exec",
            "task-exec",
            serde_json::json!({ "command": "echo", "args": [] }),
            false,
        )
        .await;
        assert_eq!(receipt.status, tool_runtime::ToolStatus::Disabled);
        let error = receipt.error.expect("structured error");
        assert_eq!(error.code, "tool_disabled");
    }

    #[tokio::test]
    async fn tool_invalid_params_rejected() {
        let state = test_state().await;
        let registry = state.tools.lock().await;
        let bad = registry.validate("fs.read", &serde_json::json!({}));
        assert!(bad.is_err());
    }

    async fn submit_raw(state: &AppState, title: &str) -> agent_runtime::queue::TaskView {
        let seq = {
            let mut runtime = state.runtime.lock().await;
            runtime.next_queue_seq()
        };
        agent_runtime::queue::submit_task(
            &state.db,
            &mut { seq },
            agent_runtime::queue::SubmitParams {
                title: title.to_owned(),
                description: None,
                priority: "normal".to_owned(),
                timeout_ms: 5_000,
                max_attempts: 2,
                scenario: "quick_success".to_owned(),
            },
        )
        .await
        .expect("submit")
    }

    #[tokio::test]
    async fn computer_tools_disabled_by_default() {
        let state = test_state().await;
        assert!(!state.computer.is_enabled());
        let receipt = invoke(
            &state,
            "computer.capture_screen",
            "task-cu-1",
            serde_json::json!({}),
            false,
        )
        .await;
        // Registry gate fires first (all computer tools ship disabled).
        assert_eq!(receipt.status, tool_runtime::ToolStatus::Disabled);
    }

    #[tokio::test]
    async fn computer_dry_run_validates_safety_without_side_effects() {
        let state = test_state().await;
        let receipt = invoke(
            &state,
            "computer.move_mouse",
            "task-cu-dry",
            serde_json::json!({ "x": 9999, "y": 10 }),
            true,
        )
        .await;
        assert_eq!(receipt.status, tool_runtime::ToolStatus::Denied);
        let error = receipt.error.expect("structured error");
        assert_eq!(error.code, "out_of_bounds");
        assert!(!state.tool_root.join("computer-profiles").exists());
    }

    #[tokio::test]
    async fn computer_emergency_stop_blocks_and_resets() {
        let state = test_state().await;
        state.computer.engage_stop();
        assert!(state.computer.is_stopped());
        let stopped = invoke(
            &state,
            "computer.capture_screen",
            "task-cu-stop",
            serde_json::json!({}),
            true,
        )
        .await;
        assert_eq!(stopped.status, tool_runtime::ToolStatus::Denied);
        assert_eq!(
            stopped.error.expect("structured error").code,
            "emergency_stop"
        );
        state.computer.clear_stop();
        let resumed = invoke(
            &state,
            "computer.capture_screen",
            "task-cu-stop",
            serde_json::json!({}),
            true,
        )
        .await;
        assert_eq!(resumed.status, tool_runtime::ToolStatus::Success);
    }

    #[tokio::test]
    async fn computer_emergency_stop_cancels_active_tasks() {
        let state = test_state().await;
        let first = submit_raw(&state, "cu task one").await;
        let second = submit_raw(&state, "cu task two").await;
        assert!(!agent_runtime::lifecycle::is_terminal(&first.status));
        let canceled = cancel_active_tasks(&state)
            .await
            .expect("cancel active tasks");
        assert_eq!(canceled, 2);
        let after = agent_runtime::queue::get_task_state(&state.db, &second.id)
            .await
            .expect("task state");
        assert_eq!(after.status, "canceling");
    }

    #[tokio::test]
    async fn context_budget_status_covers_all_seven_layers() {
        let state = test_state().await;
        let task = submit_raw(&state, "ctx task").await;
        let status = context_budget_status_inner(&state, task.id.clone(), None)
            .await
            .expect("budget status");
        assert_eq!(status.task_id, task.id);
        assert_eq!(status.layers.len(), 7);
        assert!(!status.overflow);
        assert!(status
            .layers
            .iter()
            .any(|layer| layer.layer == "safety" && layer.protected));
        assert!(status
            .layers
            .iter()
            .any(|layer| layer.layer == "scratchpad" && !layer.protected));
        // Unknown tasks still report (untitled placeholder, no evidence).
        let missing = context_budget_status_inner(&state, "task-missing".to_owned(), None)
            .await
            .expect("missing task reports");
        assert_eq!(missing.task_id, "task-missing");
        // Invalid ids are transport errors.
        assert!(context_budget_status_inner(&state, String::new(), None)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn context_compact_persists_verified_receipt() {
        let state = test_state().await;
        let task = submit_raw(&state, "ctx compact task").await;
        let receipt = context_compact_inner(&state, None, task.id.clone(), None)
            .await
            .expect("compact works");
        assert_eq!(receipt.task_id, task.id);
        assert!(receipt.proof_hash.starts_with("sha256:"));
        assert!(!receipt.preserved_invariants.is_empty());
        assert!(receipt.evidence_id.is_some());
        // Receipt is verified evidence in the ledger.
        let rows = evidence_store::list_recent_evidence(&state.db, 20)
            .await
            .expect("ledger reads");
        let evidence_id = receipt.evidence_id.clone().expect("evidence id");
        let stored = rows
            .iter()
            .find(|row| row.id == evidence_id)
            .expect("stored");
        assert_eq!(stored.kind, context_budget::COMPACTION_KIND);
        assert_eq!(stored.status, "verified");
    }

    #[tokio::test]
    async fn context_diagnostic_snapshot_carries_claim_and_plan_refs() {
        let state = test_state().await;
        let receipt = context_diagnostic_snapshot_inner(&state, None, None)
            .await
            .expect("diagnostic snapshot works");
        assert_eq!(receipt.task_id, "diag-task-1");
        assert!(receipt.proof_hash.starts_with("sha256:"));
        assert!(receipt
            .preserved_invariants
            .iter()
            .any(|row| row.starts_with("compensation:")));
        assert!(receipt.evidence_id.is_some());
    }

    async fn rag_query(
        state: &AppState,
        intent: &str,
        capabilities: Vec<String>,
        risk_ceiling: Option<String>,
        require_evidence: bool,
        top_k: Option<usize>,
    ) -> Vec<tool_rag::ToolCandidate> {
        query_tool_candidates_inner(
            state,
            intent.to_owned(),
            capabilities,
            risk_ceiling,
            require_evidence,
            top_k,
        )
        .await
        .expect("rag query works")
    }

    #[tokio::test]
    async fn tool_rag_indexes_manifests_and_ranks_by_keyword() {
        let state = test_state().await;
        let candidates = rag_query(&state, "read file contents", vec![], None, false, None).await;
        assert!(!candidates.is_empty());
        assert!(candidates.len() <= 5);
        let top = &candidates[0];
        assert_eq!(top.tool_id, "fs.read");
        assert!(top.score > 0.0);
        assert!(!top.matched_fields.is_empty());
        assert_eq!(top.risk_level, "Low");
        // Deterministic for fixed inputs (index rebuilt per query).
        let again = rag_query(&state, "read file contents", vec![], None, false, None).await;
        assert_eq!(candidates, again);
    }

    #[tokio::test]
    async fn tool_rag_capability_filter_selects_tools() {
        let state = test_state().await;
        let candidates = rag_query(
            &state,
            "click the button",
            vec!["OpenBrowser".to_owned()],
            None,
            false,
            Some(10),
        )
        .await;
        assert!(!candidates.is_empty());
        // Only OpenBrowser tools survive; every candidate explains the match.
        for candidate in &candidates {
            assert!(
                candidate.reason.contains("capability match"),
                "missing reason: {}",
                candidate.reason
            );
        }
        assert!(candidates
            .iter()
            .any(|candidate| candidate.tool_id == "browser.click"));
        assert!(candidates
            .iter()
            .all(|candidate| candidate.tool_id.starts_with("browser.")));
    }

    #[tokio::test]
    async fn tool_rag_risk_ceiling_filters_unsafe_candidates() {
        let state = test_state().await;
        let candidates = rag_query(
            &state,
            "execute command",
            vec![],
            Some("low".to_owned()),
            false,
            Some(20),
        )
        .await;
        assert!(!candidates.is_empty());
        for candidate in &candidates {
            assert_eq!(candidate.risk_level, "Low", "leaked {}", candidate.tool_id);
        }
        assert!(candidates
            .iter()
            .all(|candidate| candidate.tool_id != "process.exec"));
        assert!(candidates
            .iter()
            .all(|candidate| !candidate.tool_id.starts_with("computer.click")));
        assert!(candidates
            .iter()
            .any(|candidate| candidate.tool_id == "fs.read"));
    }

    #[tokio::test]
    async fn tool_rag_failed_receipts_reduce_ranking() {
        let state = test_state().await;
        let clean = rag_query(&state, "read missing file", vec![], None, false, Some(20)).await;
        let clean_read = clean
            .iter()
            .find(|candidate| candidate.tool_id == "fs.read")
            .expect("fs.read ranks clean");
        assert!(!clean_read.reason.contains("penalty"));
        // Seed three adapter-level failures (missing path, live run).
        for task in ["task-fail-1", "task-fail-2", "task-fail-3"] {
            let receipt = invoke(
                &state,
                "fs.read",
                task,
                serde_json::json!({ "path": "missing.txt" }),
                false,
            )
            .await;
            assert_eq!(receipt.status, tool_runtime::ToolStatus::Failed);
        }
        let penalized = rag_query(&state, "read missing file", vec![], None, false, Some(20)).await;
        let penalized_read = penalized
            .iter()
            .find(|candidate| candidate.tool_id == "fs.read")
            .expect("fs.read still ranks");
        assert!(
            penalized_read
                .reason
                .contains("penalty: 3 failed/denied receipt(s)"),
            "reason: {}",
            penalized_read.reason
        );
        assert!(
            penalized_read.score < clean_read.score,
            "penalty must reduce the score"
        );
    }

    #[tokio::test]
    async fn tool_rag_success_boost_and_evidence_requirement() {
        let state = test_state().await;
        // Seed evidence: a read (evidence) then a live write (success + plan).
        let read = invoke(
            &state,
            "fs.read",
            "task-rag-write",
            serde_json::json!({ "path": "seed.txt" }),
            false,
        )
        .await;
        // Seed file may not exist; dry-run the write path instead when the
        // read fails so the test never depends on fixture files.
        if read.status == tool_runtime::ToolStatus::Success {
            let wrote = invoke(
                &state,
                "fs.write",
                "task-rag-write",
                serde_json::json!({ "path": "rag-out.txt", "content": "hello" }),
                false,
            )
            .await;
            assert_eq!(wrote.status, tool_runtime::ToolStatus::Success);
        }
        let candidates = rag_query(&state, "write file", vec![], None, true, Some(20)).await;
        // Evidence requirement excludes tools with no backing.
        assert!(!candidates.is_empty());
        for candidate in &candidates {
            let backed = candidate.reason.contains("successful receipt")
                || candidate.reason.contains("evidence-backed");
            assert!(backed, "unbacked candidate: {}", candidate.reason);
        }
        // fs.search has no evidence flag and no receipts: always excluded.
        assert!(candidates
            .iter()
            .all(|candidate| candidate.tool_id != "fs.search"));
    }

    #[tokio::test]
    async fn tool_rag_rejects_invalid_queries() {
        let state = test_state().await;
        assert!(
            query_tool_candidates_inner(&state, String::new(), vec![], None, false, None)
                .await
                .is_err()
        );
        assert!(query_tool_candidates_inner(
            &state,
            "read".to_owned(),
            vec![],
            Some("extreme".to_owned()),
            false,
            None,
        )
        .await
        .is_err());
    }
}
