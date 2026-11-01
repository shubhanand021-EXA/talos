//! Policy-gated execution: every invocation becomes an `ActionRequest`,
//! is audited, and only runs when the broker allows.
//!
//! Flow: validate → enabled-check → build request → record audit → evaluate
//! with live ledger context → deny/disabled short-circuit with structured
//! errors → (write: compensation plan first) → adapter → receipt persisted as
//! evidence (verified). Failed invocations produce structured errors; nothing
//! bypasses the broker.

use evidence_store::EvidenceDb;
use policy_broker::{PolicyContext, PolicyDecision, PolicyEngine};
use talos_common::{now_iso, Result, TalosError};
use uuid::Uuid;

use crate::adapters::{ExecutionMode, SandboxConfig};
use crate::model::*;
use crate::registry::{ToolRegistry, ValidatedParams};

fn err(code: &str, message: String) -> ToolErrorDetail {
    ToolErrorDetail::new(code, message)
}

fn storage(message: String) -> TalosError {
    TalosError::Storage(message)
}

/// Outcome of one gated invocation.
#[derive(Debug, Clone)]
pub struct GatedOutcome {
    /// Receipt (always present, even for denials).
    pub receipt: ToolReceipt,
    /// Whether the adapter actually ran.
    pub executed: bool,
}

/// Build the policy request for an invocation (risk from the manifest).
fn action_request(
    manifest: &ToolManifest,
    invocation: &ToolInvocation,
    receipt_id: &str,
) -> policy_broker::ActionRequest {
    let (risk_level, reversible) = risk_for(&manifest.id);
    // High-risk browser/computer mutations (form submit, purchase, settings,
    // upload, destructive) escalate to human review via an explicit flag.
    let (risk_level, reversible) = if let Some(high) = invocation
        .params
        .get("highRisk")
        .and_then(|value| value.as_bool())
    {
        if high
            && matches!(
                manifest.id.as_str(),
                crate::model::BROWSER_CLICK
                    | crate::model::BROWSER_TYPE
                    | crate::model::BROWSER_SELECT
                    | crate::model::COMPUTER_CLICK
                    | crate::model::COMPUTER_DOUBLE_CLICK
                    | crate::model::COMPUTER_DRAG
                    | crate::model::COMPUTER_TYPE_TEXT
                    | crate::model::COMPUTER_HOTKEY
            )
        {
            (policy_broker::RiskLevel::High, false)
        } else {
            (risk_level, reversible)
        }
    } else {
        (risk_level, reversible)
    };
    let target = match manifest.id.as_str() {
        FS_READ | FS_LIST | FS_SEARCH | FS_WRITE => invocation
            .params
            .get("path")
            .and_then(|value| value.as_str())
            .unwrap_or("(sandbox)")
            .to_owned(),
        crate::model::BROWSER_NAVIGATE
        | crate::model::BROWSER_EXTRACT
        | crate::model::BROWSER_SCREENSHOT
        | crate::model::BROWSER_SNAPSHOT => invocation
            .params
            .get("url")
            .and_then(|value| value.as_str())
            .unwrap_or("(browser)")
            .to_owned(),
        crate::model::BROWSER_CLICK
        | crate::model::BROWSER_TYPE
        | crate::model::BROWSER_SCROLL
        | crate::model::BROWSER_SELECT => invocation
            .params
            .get("selector")
            .and_then(|value| value.as_str())
            .unwrap_or("(browser)")
            .to_owned(),
        crate::model::COMPUTER_MOVE_MOUSE
        | crate::model::COMPUTER_CLICK
        | crate::model::COMPUTER_DOUBLE_CLICK
        | crate::model::COMPUTER_DRAG
        | crate::model::COMPUTER_TYPE_TEXT
        | crate::model::COMPUTER_HOTKEY
        | crate::model::COMPUTER_SCROLL
        | crate::model::COMPUTER_CAPTURE_SCREEN
        | crate::model::COMPUTER_LOCATE_ELEMENT => invocation
            .params
            .get("selector")
            .and_then(|value| value.as_str())
            .or_else(|| {
                invocation
                    .params
                    .get("appHint")
                    .and_then(|value| value.as_str())
            })
            .map(|value| format!("computer:{value}"))
            .unwrap_or("(computer)".to_owned()),
        PROCESS_EXEC => invocation
            .params
            .get("command")
            .and_then(|value| value.as_str())
            .unwrap_or("(command)")
            .to_owned(),
        _ => manifest.id.clone(),
    };
    policy_broker::ActionRequest {
        id: format!("tool-{receipt_id}"),
        agent_id: invocation.agent_id.clone(),
        task_id: invocation.task_id.clone(),
        capability: capability_for(&manifest.id),
        intent: format!("tool {} via Talos tool framework", manifest.id),
        target,
        payload_json: invocation.params.clone(),
        risk_level,
        reversible,
        requested_at: chrono::Utc::now(),
    }
}

/// Record the audit trail row for an evaluated request.
async fn audit(
    db: &EvidenceDb,
    request: &policy_broker::ActionRequest,
    evaluation: &policy_broker::PolicyEvaluation,
) -> Result<()> {
    evidence_store::record_action_request(
        db,
        &evidence_store::NewActionRecord {
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
        },
        "talos-policy-broker/1",
        &format!("{:?}", evaluation.decision),
        &evaluation.reason,
        &evaluation.required_checks,
    )
    .await
}

/// Persist a receipt as evidence (kind `tool_receipt`, immediately verified).
async fn persist_receipt(db: &EvidenceDb, receipt: &ToolReceipt) -> Result<String> {
    let evidence_id = format!("tool-evidence-{}", receipt.id);
    evidence_store::insert_evidence(
        db,
        evidence_store::NewEvidence {
            id: evidence_id.clone(),
            task_id: Some(receipt.task_id.clone()),
            agent_id: None,
            session_id: Some("session-local".to_owned()),
            kind: "tool_receipt".to_owned(),
            label: Some(format!("{} {}", receipt.tool_id, receipt.id)),
            summary: Some(format!(
                "{:?} {} (dry_run={})",
                receipt.status, receipt.tool_id, receipt.dry_run
            )),
            uri: None,
            mime_type: None,
            payload: serde_json::to_value(receipt)
                .map_err(|err| storage(format!("receipt serialize: {err}")))?,
        },
    )
    .await
    .map(|_| evidence_id.clone())?;
    evidence_store::transition_evidence_status(db, &evidence_id, "verified").await?;
    Ok(evidence_id)
}

/// Invoke a tool through policy, evidence, and compensation gates.
pub async fn invoke(
    db: &EvidenceDb,
    engine: &dyn PolicyEngine,
    registry: &ToolRegistry,
    config: &SandboxConfig,
    invocation: ToolInvocation,
) -> Result<GatedOutcome> {
    validate_ids(
        &invocation.tool_id,
        &invocation.task_id,
        &invocation.agent_id,
    )
    .map_err(|err| storage(err.to_string()))?;
    let manifest = registry
        .get(&invocation.tool_id)
        .cloned()
        .ok_or_else(|| storage("unknown tool".to_owned()))?;
    let validated = registry
        .validate(&invocation.tool_id, &invocation.params)
        .map_err(|err| storage(err.to_string()))?;
    let receipt_id = format!("receipt-{}", Uuid::new_v4());
    let request = action_request(&manifest, &invocation, &receipt_id);
    invoke_with_request(
        db, engine, registry, config, invocation, manifest, validated, request, receipt_id,
    )
    .await
}

/// Inner invocation once the action request is built (compensation first for writes).
#[allow(clippy::too_many_arguments)]
async fn invoke_with_request(
    db: &EvidenceDb,
    engine: &dyn PolicyEngine,
    registry: &ToolRegistry,
    config: &SandboxConfig,
    invocation: ToolInvocation,
    manifest: ToolManifest,
    validated: ValidatedParams,
    request: policy_broker::ActionRequest,
    receipt_id: String,
) -> Result<GatedOutcome> {
    // Live execution of a disabled tool short-circuits (dry-run still plans).
    if !invocation.dry_run && !registry.is_enabled(&manifest.id) {
        let evaluation = engine.evaluate_with_context(
            &request,
            &PolicyContext {
                has_evidence: false,
                has_compensation_plan: false,
            },
        );
        audit(db, &request, &evaluation).await?;
        let mut receipt = ToolReceipt {
            id: receipt_id,
            tool_id: manifest.id.clone(),
            task_id: invocation.task_id.clone(),
            dry_run: false,
            status: ToolStatus::Disabled,
            output: serde_json::json!({}),
            error: Some(err(
                "tool_disabled",
                format!("{} is disabled by default", manifest.id),
            )),
            policy_decision: evaluation.decision,
            policy_reason: evaluation.reason.clone(),
            evidence_id: None,
            compensation_plan_id: None,
            executed_at: now_iso(),
        };
        receipt.evidence_id = Some(persist_receipt(db, &receipt).await?);
        return Ok(GatedOutcome {
            receipt,
            executed: false,
        });
    }

    // Computer-use safety envelope: runs before policy evaluation so an
    // engaged emergency stop, a disabled envelope, rate exhaustion, bounds
    // violations, and dangerous zones deny the action with a structured
    // safety code. Dry-runs are still planned when only the enable flag or
    // rate limiter would block; the stop latch blocks everything.
    let computer_warnings: Vec<String> = if manifest.id.starts_with("computer.") {
        match computer_safety_check(config, &validated, invocation.dry_run).await {
            Ok(warnings) => warnings,
            Err(violation) => {
                let mut receipt = ToolReceipt {
                    id: receipt_id,
                    tool_id: manifest.id.clone(),
                    task_id: invocation.task_id.clone(),
                    dry_run: invocation.dry_run,
                    status: ToolStatus::Denied,
                    output: serde_json::json!({}),
                    error: Some(err(&violation.code, violation.message.clone())),
                    policy_decision: PolicyDecision::Deny,
                    policy_reason: violation.message,
                    evidence_id: None,
                    compensation_plan_id: None,
                    executed_at: now_iso(),
                };
                receipt.evidence_id = Some(persist_receipt(db, &receipt).await?);
                return Ok(GatedOutcome {
                    receipt,
                    executed: false,
                });
            }
        }
    } else {
        Vec::new()
    };

    let has_evidence = evidence_store::has_evidence_for_task(db, &invocation.task_id).await?;
    let has_plan = evidence_store::has_compensation_plan(db, &request.id).await?;
    let evaluation = engine.evaluate_with_context(
        &request,
        &PolicyContext {
            has_evidence,
            has_compensation_plan: has_plan,
        },
    );
    audit(db, &request, &evaluation).await?;

    if evaluation.decision != PolicyDecision::Allow {
        let mut receipt = ToolReceipt {
            id: receipt_id,
            tool_id: manifest.id.clone(),
            task_id: invocation.task_id.clone(),
            dry_run: invocation.dry_run,
            status: ToolStatus::Denied,
            output: serde_json::json!({}),
            error: Some(err(
                "policy_denied",
                format!(
                    "policy {:?}: {} [{}]",
                    evaluation.decision,
                    evaluation.reason,
                    evaluation.required_checks.join(", ")
                ),
            )),
            policy_decision: evaluation.decision,
            policy_reason: evaluation.reason.clone(),
            evidence_id: None,
            compensation_plan_id: None,
            executed_at: now_iso(),
        };
        receipt.evidence_id = Some(persist_receipt(db, &receipt).await?);
        return Ok(GatedOutcome {
            receipt,
            executed: false,
        });
    }

    // Allowed: dry-run plans, live executes (writes compensate first).
    let mut compensation_plan_id: Option<String> = None;
    if !invocation.dry_run && manifest.id == FS_WRITE {
        let backup = probe_backup(config, write_path(&validated)).await;
        let plan_id = format!("plan-{}", request.id);
        if !evidence_store::has_compensation_plan(db, &request.id).await? {
            evidence_store::create_compensation_plan(
                db,
                evidence_store::NewCompensationPlan {
                    id: plan_id.clone(),
                    action_request_id: request.id.clone(),
                    strategy: "backup_restore".to_owned(),
                    reason: format!("tool fs.write may overwrite {}", request.target),
                    steps: vec![evidence_store::NewCompensationStep {
                        action_type: "backup_restore".to_owned(),
                        payload: serde_json::json!({
                            "target": request.target,
                            "backup": backup,
                        }),
                    }],
                },
            )
            .await?;
        }
        compensation_plan_id = Some(plan_id);
    }
    let mode = if invocation.dry_run {
        ExecutionMode::DryRun
    } else {
        ExecutionMode::Live
    };
    let (status, output, error, _) = run_adapter(
        mode,
        config,
        &invocation.task_id,
        &manifest,
        &validated,
        &invocation.params,
    )
    .await?;
    // Surface non-blocking safety warnings (terminal/payment zones) in the
    // receipt output so the UI can display them next to the result.
    let mut output = output;
    if !computer_warnings.is_empty() {
        if let Some(object) = output.as_object_mut() {
            object.insert(
                "warnings".to_owned(),
                serde_json::Value::Array(
                    computer_warnings
                        .into_iter()
                        .map(serde_json::Value::String)
                        .collect(),
                ),
            );
        }
    }
    let mut receipt = ToolReceipt {
        id: receipt_id,
        tool_id: manifest.id.clone(),
        task_id: invocation.task_id.clone(),
        dry_run: invocation.dry_run,
        status,
        output,
        error,
        policy_decision: evaluation.decision,
        policy_reason: evaluation.reason.clone(),
        evidence_id: None,
        compensation_plan_id,
        executed_at: now_iso(),
    };
    receipt.evidence_id = Some(persist_receipt(db, &receipt).await?);
    Ok(GatedOutcome {
        receipt,
        executed: !invocation.dry_run,
    })
}

async fn run_adapter(
    mode: ExecutionMode,
    config: &SandboxConfig,
    task_id: &str,
    manifest: &ToolManifest,
    validated: &ValidatedParams,
    params: &serde_json::Value,
) -> Result<(
    ToolStatus,
    serde_json::Value,
    Option<ToolErrorDetail>,
    Option<String>,
)> {
    use crate::tools_browser as browser;
    use crate::tools_exec as exec;
    use crate::tools_fs as fs;
    use computer_use::mock as computer;
    match (manifest.id.as_str(), validated) {
        (crate::model::FS_READ, ValidatedParams::FsRead { path }) => {
            if mode == ExecutionMode::DryRun {
                return Ok((
                    ToolStatus::Success,
                    serde_json::json!({ "path": path, "planned": true }),
                    None,
                    None,
                ));
            }
            match fs::fs_read(config, path).await {
                Ok(output) => Ok((ToolStatus::Success, output, None, None)),
                Err(error) => Ok((
                    ToolStatus::Failed,
                    serde_json::json!({}),
                    Some(err("io_error", error.to_string())),
                    None,
                )),
            }
        }
        (crate::model::FS_LIST, ValidatedParams::FsList { path, depth }) => {
            if mode == ExecutionMode::DryRun {
                return Ok((
                    ToolStatus::Success,
                    serde_json::json!({ "path": path, "depth": depth, "planned": true }),
                    None,
                    None,
                ));
            }
            match fs::fs_list(config, path, *depth).await {
                Ok(output) => Ok((ToolStatus::Success, output, None, None)),
                Err(error) => Ok((
                    ToolStatus::Failed,
                    serde_json::json!({}),
                    Some(err("io_error", error.to_string())),
                    None,
                )),
            }
        }
        (
            crate::model::FS_SEARCH,
            ValidatedParams::FsSearch {
                query,
                path,
                max_matches,
            },
        ) => {
            if mode == ExecutionMode::DryRun {
                return Ok((
                    ToolStatus::Success,
                    serde_json::json!({ "query": query, "path": path, "planned": true }),
                    None,
                    None,
                ));
            }
            match fs::fs_search(config, query, path, *max_matches).await {
                Ok(output) => Ok((ToolStatus::Success, output, None, None)),
                Err(error) => Ok((
                    ToolStatus::Failed,
                    serde_json::json!({}),
                    Some(err("io_error", error.to_string())),
                    None,
                )),
            }
        }
        (
            crate::model::FS_WRITE,
            ValidatedParams::FsWrite {
                path,
                content,
                create_parents,
            },
        ) => {
            if mode == ExecutionMode::DryRun {
                return Ok((
                    ToolStatus::Success,
                    fs::fs_write_plan(path, content, false),
                    None,
                    None,
                ));
            }
            match fs::fs_write(config, path, content, *create_parents).await {
                Ok(output) => Ok((ToolStatus::Success, output, None, None)),
                Err(error) => Ok((
                    ToolStatus::Failed,
                    serde_json::json!({}),
                    Some(err("io_error", error.to_string())),
                    None,
                )),
            }
        }
        (
            crate::model::PROCESS_EXEC,
            ValidatedParams::ProcessExec {
                command,
                args,
                timeout_ms,
            },
        ) => {
            if mode == ExecutionMode::DryRun {
                return Ok((
                    ToolStatus::Success,
                    exec::exec_plan(command, args, *timeout_ms),
                    None,
                    None,
                ));
            }
            match exec::exec_live(config, command, args, *timeout_ms).await {
                Ok(output) => Ok((ToolStatus::Success, output, None, None)),
                Err(exec_err) => {
                    let message = exec_err.to_string();
                    let code = if message.contains("disabled") {
                        "tool_disabled"
                    } else if message.contains("allowlisted") {
                        "not_allowlisted"
                    } else if message.contains("timed out") {
                        "timeout"
                    } else {
                        "io_error"
                    };
                    let status = if code == "tool_disabled" {
                        ToolStatus::Disabled
                    } else {
                        ToolStatus::Failed
                    };
                    Ok((
                        status,
                        serde_json::json!({}),
                        Some(err(code, message)),
                        None,
                    ))
                }
            }
        }
        (
            crate::model::BROWSER_NAVIGATE,
            ValidatedParams::BrowserNavigate {
                url, timeout_ms, ..
            },
        ) => {
            if mode == ExecutionMode::DryRun {
                return Ok((
                    ToolStatus::Success,
                    browser::navigate_plan(url, *timeout_ms),
                    None,
                    None,
                ));
            }
            if crate::browser_worker::live_enabled() {
                let (status, output, error) =
                    crate::browser_worker::execute_tool_call(&manifest.id, task_id, params).await;
                return Ok((status, output, error, None));
            }
            match browser::navigate_live(config, task_id, url, *timeout_ms).await {
                Ok(output) => Ok((ToolStatus::Success, output, None, None)),
                Err(error) => Ok((
                    ToolStatus::Failed,
                    serde_json::json!({}),
                    Some(err("io_error", error.to_string())),
                    None,
                )),
            }
        }
        (
            crate::model::BROWSER_CLICK,
            ValidatedParams::BrowserClick {
                url,
                selector,
                high_risk,
                high_risk_kind,
                ..
            },
        ) => {
            if mode == ExecutionMode::DryRun {
                match browser::click_plan(
                    url.as_deref(),
                    selector,
                    10_000,
                    *high_risk,
                    high_risk_kind.as_deref(),
                ) {
                    Ok(output) => Ok((ToolStatus::Success, output, None, None)),
                    Err(error) => Ok((
                        ToolStatus::Failed,
                        serde_json::json!({}),
                        Some(err("invalid_params", error.to_string())),
                        None,
                    )),
                }
            } else {
                if crate::browser_worker::live_enabled() {
                    let (status, output, error) =
                        crate::browser_worker::execute_tool_call(&manifest.id, task_id, params)
                            .await;
                    return Ok((status, output, error, None));
                }
                match browser::click_live(
                    config,
                    task_id,
                    url.as_deref(),
                    selector,
                    *high_risk,
                    high_risk_kind.as_deref(),
                )
                .await
                {
                    Ok(output) => Ok((ToolStatus::Success, output, None, None)),
                    Err(error) => Ok((
                        ToolStatus::Failed,
                        serde_json::json!({}),
                        Some(err("io_error", error.to_string())),
                        None,
                    )),
                }
            }
        }
        (
            crate::model::BROWSER_TYPE,
            ValidatedParams::BrowserType {
                url,
                selector,
                text,
                submit,
                timeout_ms,
                high_risk,
                high_risk_kind,
                ..
            },
        ) => {
            if mode == ExecutionMode::DryRun {
                match browser::type_plan(
                    url.as_deref(),
                    selector,
                    text,
                    *submit,
                    *timeout_ms,
                    *high_risk,
                    high_risk_kind.as_deref(),
                ) {
                    Ok(output) => Ok((ToolStatus::Success, output, None, None)),
                    Err(error) => Ok((
                        ToolStatus::Failed,
                        serde_json::json!({}),
                        Some(err("invalid_params", error.to_string())),
                        None,
                    )),
                }
            } else {
                if crate::browser_worker::live_enabled() {
                    let (status, output, error) =
                        crate::browser_worker::execute_tool_call(&manifest.id, task_id, params)
                            .await;
                    return Ok((status, output, error, None));
                }
                match browser::type_live(
                    config,
                    task_id,
                    url.as_deref(),
                    selector,
                    text,
                    *submit,
                    *high_risk,
                    high_risk_kind.as_deref(),
                )
                .await
                {
                    Ok(output) => Ok((ToolStatus::Success, output, None, None)),
                    Err(error) => Ok((
                        ToolStatus::Failed,
                        serde_json::json!({}),
                        Some(err("io_error", error.to_string())),
                        None,
                    )),
                }
            }
        }
        (
            crate::model::BROWSER_SCROLL,
            ValidatedParams::BrowserScroll {
                url,
                direction,
                pixels,
                ..
            },
        ) => {
            if mode == ExecutionMode::DryRun {
                return Ok((
                    ToolStatus::Success,
                    browser::scroll_plan(url.as_deref(), direction, *pixels),
                    None,
                    None,
                ));
            }
            if crate::browser_worker::live_enabled() {
                let (status, output, error) =
                    crate::browser_worker::execute_tool_call(&manifest.id, task_id, params).await;
                return Ok((status, output, error, None));
            }
            match browser::scroll_live(config, task_id, url.as_deref(), direction, *pixels).await {
                Ok(output) => Ok((ToolStatus::Success, output, None, None)),
                Err(error) => Ok((
                    ToolStatus::Failed,
                    serde_json::json!({}),
                    Some(err("io_error", error.to_string())),
                    None,
                )),
            }
        }
        (
            crate::model::BROWSER_SELECT,
            ValidatedParams::BrowserSelect {
                url,
                selector,
                value,
                ..
            },
        ) => {
            if mode == ExecutionMode::DryRun {
                match browser::select_plan(url.as_deref(), selector, value.as_deref(), 10_000) {
                    Ok(output) => Ok((ToolStatus::Success, output, None, None)),
                    Err(error) => Ok((
                        ToolStatus::Failed,
                        serde_json::json!({}),
                        Some(err("invalid_params", error.to_string())),
                        None,
                    )),
                }
            } else {
                if crate::browser_worker::live_enabled() {
                    let (status, output, error) =
                        crate::browser_worker::execute_tool_call(&manifest.id, task_id, params)
                            .await;
                    return Ok((status, output, error, None));
                }
                match browser::select_live(
                    config,
                    task_id,
                    url.as_deref(),
                    selector,
                    value.as_deref(),
                )
                .await
                {
                    Ok(output) => Ok((ToolStatus::Success, output, None, None)),
                    Err(error) => Ok((
                        ToolStatus::Failed,
                        serde_json::json!({}),
                        Some(err("io_error", error.to_string())),
                        None,
                    )),
                }
            }
        }
        (
            crate::model::BROWSER_EXTRACT,
            ValidatedParams::BrowserExtract {
                url,
                selector,
                max_chars,
                ..
            },
        ) => {
            // Read-only: dry-run shares the deterministic rendering; live
            // routes to the worker only when explicitly opted in.
            if mode == ExecutionMode::Live && crate::browser_worker::live_enabled() {
                let (status, output, error) =
                    crate::browser_worker::execute_tool_call(&manifest.id, task_id, params).await;
                return Ok((status, output, error, None));
            }
            let _ = mode;
            Ok((
                ToolStatus::Success,
                browser::extract_live(url, selector.as_deref(), *max_chars),
                None,
                None,
            ))
        }
        (
            crate::model::BROWSER_SCREENSHOT,
            ValidatedParams::BrowserScreenshot { url, full_page, .. },
        ) => {
            // Read-only hash: dry-run shares the deterministic rendering; live
            // routes to the worker only when explicitly opted in.
            if mode == ExecutionMode::Live && crate::browser_worker::live_enabled() {
                let (status, output, error) =
                    crate::browser_worker::execute_tool_call(&manifest.id, task_id, params).await;
                return Ok((status, output, error, None));
            }
            let _ = mode;
            Ok((
                ToolStatus::Success,
                browser::screenshot_live(url, *full_page),
                None,
                None,
            ))
        }
        (
            crate::model::BROWSER_SNAPSHOT,
            ValidatedParams::BrowserSnapshot { url, selector, .. },
        ) => {
            // Read-only: dry-run shares the deterministic rendering; live
            // routes to the worker only when explicitly opted in.
            if mode == ExecutionMode::Live && crate::browser_worker::live_enabled() {
                let (status, output, error) =
                    crate::browser_worker::execute_tool_call(&manifest.id, task_id, params).await;
                return Ok((status, output, error, None));
            }
            let _ = mode;
            Ok((
                ToolStatus::Success,
                browser::snapshot_live(url, selector.as_deref()),
                None,
                None,
            ))
        }
        (crate::model::BROWSER_SESSION, ValidatedParams::BrowserSession { operation }) => {
            if mode == ExecutionMode::DryRun {
                return Ok((
                    ToolStatus::Success,
                    browser::session_plan(operation),
                    None,
                    None,
                ));
            }
            if crate::browser_worker::live_enabled() {
                let (status, output, error) =
                    crate::browser_worker::execute_tool_call(&manifest.id, task_id, params).await;
                return Ok((status, output, error, None));
            }
            Ok((
                ToolStatus::Success,
                browser::session_mock(task_id, operation),
                None,
                None,
            ))
        }
        (crate::model::COMPUTER_MOVE_MOUSE, ValidatedParams::ComputerMoveMouse { x, y, .. }) => {
            if mode == ExecutionMode::DryRun {
                return Ok((
                    ToolStatus::Success,
                    computer::MockComputer::move_plan(*x, *y),
                    None,
                    None,
                ));
            }
            match computer::MockComputer::move_live(&config.root, task_id, *x, *y).await {
                Ok(output) => Ok((ToolStatus::Success, output, None, None)),
                Err(error) => Ok(mock_failure("computer.move_mouse", error)),
            }
        }
        (crate::model::COMPUTER_CLICK, ValidatedParams::ComputerClick { x, y, selector, .. })
        | (
            crate::model::COMPUTER_DOUBLE_CLICK,
            ValidatedParams::ComputerDoubleClick { x, y, selector, .. },
        ) => {
            let double = manifest.id == crate::model::COMPUTER_DOUBLE_CLICK;
            let action = if double {
                "computer.double_click"
            } else {
                "computer.click"
            };
            if mode == ExecutionMode::DryRun {
                match computer::MockComputer::click_plan(selector.as_deref(), *x, *y) {
                    Ok(output) => Ok((ToolStatus::Success, output, None, None)),
                    Err(error) => Ok((
                        ToolStatus::Failed,
                        serde_json::json!({}),
                        Some(err("invalid_params", error.to_string())),
                        None,
                    )),
                }
            } else {
                let min_confidence = config.computer.config().min_confidence;
                match computer::MockComputer::click_live(
                    &config.root,
                    task_id,
                    selector.as_deref(),
                    *x,
                    *y,
                    double,
                    min_confidence,
                )
                .await
                {
                    Ok(output) => Ok((ToolStatus::Success, output, None, None)),
                    Err(error) => Ok(mock_failure(action, error)),
                }
            }
        }
        (crate::model::COMPUTER_DRAG, ValidatedParams::ComputerDrag { x, y, x2, y2, .. }) => {
            if mode == ExecutionMode::DryRun {
                return Ok((
                    ToolStatus::Success,
                    serde_json::json!({
                        "from": { "x": x, "y": y },
                        "to": { "x": x2, "y": y2 },
                        "planned": true,
                        "simulated": true,
                    }),
                    None,
                    None,
                ));
            }
            match computer::MockComputer::drag_live(&config.root, task_id, *x, *y, *x2, *y2).await {
                Ok(output) => Ok((ToolStatus::Success, output, None, None)),
                Err(error) => Ok(mock_failure("computer.drag", error)),
            }
        }
        (
            crate::model::COMPUTER_TYPE_TEXT,
            ValidatedParams::ComputerTypeText { text, selector, .. },
        ) => {
            if mode == ExecutionMode::DryRun {
                match computer::MockComputer::type_plan(text, selector.as_deref()) {
                    Ok(output) => Ok((ToolStatus::Success, output, None, None)),
                    Err(error) => Ok((
                        ToolStatus::Failed,
                        serde_json::json!({}),
                        Some(err("invalid_params", error.to_string())),
                        None,
                    )),
                }
            } else {
                let min_confidence = config.computer.config().min_confidence;
                match computer::MockComputer::type_live(
                    &config.root,
                    task_id,
                    selector.as_deref(),
                    text,
                    min_confidence,
                )
                .await
                {
                    Ok(output) => Ok((ToolStatus::Success, output, None, None)),
                    Err(error) => Ok(mock_failure("computer.type_text", error)),
                }
            }
        }
        (crate::model::COMPUTER_HOTKEY, ValidatedParams::ComputerHotkey { keys, .. }) => {
            if mode == ExecutionMode::DryRun {
                match computer::MockComputer::hotkey_plan(keys) {
                    Ok(output) => Ok((ToolStatus::Success, output, None, None)),
                    Err(error) => Ok((
                        ToolStatus::Failed,
                        serde_json::json!({}),
                        Some(err("invalid_params", error.to_string())),
                        None,
                    )),
                }
            } else {
                match computer::MockComputer::hotkey_live(&config.root, task_id, keys).await {
                    Ok(output) => Ok((ToolStatus::Success, output, None, None)),
                    Err(error) => Ok(mock_failure("computer.hotkey", error)),
                }
            }
        }
        (
            crate::model::COMPUTER_SCROLL,
            ValidatedParams::ComputerScroll {
                x,
                y,
                direction,
                pixels,
                ..
            },
        ) => {
            if mode == ExecutionMode::DryRun {
                return Ok((
                    ToolStatus::Success,
                    serde_json::json!({
                        "direction": direction,
                        "pixels": pixels,
                        "at": { "x": x, "y": y },
                        "planned": true,
                        "simulated": true,
                    }),
                    None,
                    None,
                ));
            }
            match computer::MockComputer::scroll_live(
                &config.root,
                task_id,
                *x,
                *y,
                direction,
                *pixels,
            )
            .await
            {
                Ok(output) => Ok((ToolStatus::Success, output, None, None)),
                Err(error) => Ok(mock_failure("computer.scroll", error)),
            }
        }
        (crate::model::COMPUTER_CAPTURE_SCREEN, ValidatedParams::ComputerCaptureScreen { .. }) => {
            // Read-only: dry-run and live share the deterministic rendering.
            let _ = mode;
            Ok((
                ToolStatus::Success,
                computer::MockComputer::capture_live(task_id),
                None,
                None,
            ))
        }
        (
            crate::model::COMPUTER_LOCATE_ELEMENT,
            ValidatedParams::ComputerLocateElement { selector },
        ) => {
            // Read-only: dry-run and live share the deterministic rendering,
            // but confidence still gates (no silent mis-targeting).
            let _ = mode;
            let min_confidence = config.computer.config().min_confidence;
            match computer::MockComputer::locate_live(selector, min_confidence) {
                Ok(output) => Ok((ToolStatus::Success, output, None, None)),
                Err(error) => Ok(mock_failure("computer.locate_element", error)),
            }
        }
        _ => Ok((
            ToolStatus::Failed,
            serde_json::json!({}),
            Some(err("invalid_params", "params do not match tool".to_owned())),
            None,
        )),
    }
}

/// Map a mock computer failure to a receipt tuple: low-confidence resolutions
/// deny (safety gate), everything else fails as an IO error.
fn mock_failure(
    action: &str,
    error: TalosError,
) -> (
    ToolStatus,
    serde_json::Value,
    Option<ToolErrorDetail>,
    Option<String>,
) {
    let message = error.to_string();
    if message.contains("low confidence") {
        (
            ToolStatus::Denied,
            serde_json::json!({}),
            Some(err("low_confidence", message)),
            None,
        )
    } else {
        (
            ToolStatus::Failed,
            serde_json::json!({}),
            Some(err("io_error", format!("{action}: {message}"))),
            None,
        )
    }
}

/// Best-effort backup probe for compensation metadata (never fails the plan).
async fn probe_backup(config: &SandboxConfig, path: &str) -> serde_json::Value {
    use crate::adapters::{contained_path, join_under_root};
    let Ok(joined) = join_under_root(&config.root, path) else {
        return serde_json::json!({ "existed": false });
    };
    let Ok(full) = contained_path(&config.root, &joined).await else {
        return serde_json::json!({ "existed": false });
    };
    match tokio::fs::read(&full).await {
        Ok(bytes) => {
            let capped: Vec<u8> = bytes.iter().take(4096).copied().collect();
            serde_json::json!({ "existed": true, "bytes": bytes.len(), "preview": String::from_utf8_lossy(&capped).into_owned() })
        }
        Err(_) => serde_json::json!({ "existed": false }),
    }
}

/// Sandbox-relative write path for compensation probing.
fn write_path(validated: &ValidatedParams) -> &str {
    match validated {
        ValidatedParams::FsWrite { path, .. } => path,
        _ => ".",
    }
}

/// Run the computer-use safety envelope for one validated invocation.
///
/// Returns the non-blocking warnings on success; violations (emergency stop,
/// disabled envelope, rate limit, out-of-bounds, dangerous zones) become
/// `SafetyViolation`s that the caller turns into a denied receipt.
async fn computer_safety_check(
    config: &SandboxConfig,
    validated: &ValidatedParams,
    dry_run: bool,
) -> std::result::Result<Vec<String>, computer_use::SafetyViolation> {
    use computer_use::safety::SafetyInput;
    let input = match validated {
        ValidatedParams::ComputerMoveMouse {
            x,
            y,
            app_hint,
            window_hint,
        } => SafetyInput {
            x: Some(*x),
            y: Some(*y),
            selector: None,
            app_hint: app_hint.clone(),
            window_hint: window_hint.clone(),
            dry_run,
            ..SafetyInput::default()
        },
        ValidatedParams::ComputerClick {
            x,
            y,
            selector,
            app_hint,
            window_hint,
            high_risk,
            ..
        }
        | ValidatedParams::ComputerDoubleClick {
            x,
            y,
            selector,
            app_hint,
            window_hint,
            high_risk,
            ..
        } => SafetyInput {
            x: *x,
            y: *y,
            selector: selector.clone(),
            app_hint: app_hint.clone(),
            window_hint: window_hint.clone(),
            high_risk: *high_risk,
            dry_run,
            ..SafetyInput::default()
        },
        ValidatedParams::ComputerDrag {
            x,
            y,
            x2,
            y2,
            app_hint,
            window_hint,
            high_risk,
        } => SafetyInput {
            x: Some(*x),
            y: Some(*y),
            x2: Some(*x2),
            y2: Some(*y2),
            app_hint: app_hint.clone(),
            window_hint: window_hint.clone(),
            high_risk: *high_risk,
            dry_run,
            ..SafetyInput::default()
        },
        ValidatedParams::ComputerTypeText {
            x,
            y,
            selector,
            app_hint,
            window_hint,
            high_risk,
            ..
        } => SafetyInput {
            x: *x,
            y: *y,
            selector: selector.clone(),
            app_hint: app_hint.clone(),
            window_hint: window_hint.clone(),
            high_risk: *high_risk,
            dry_run,
            ..SafetyInput::default()
        },
        ValidatedParams::ComputerHotkey {
            app_hint,
            window_hint,
            high_risk,
            ..
        } => SafetyInput {
            app_hint: app_hint.clone(),
            window_hint: window_hint.clone(),
            high_risk: *high_risk,
            dry_run,
            ..SafetyInput::default()
        },
        ValidatedParams::ComputerScroll {
            x,
            y,
            app_hint,
            window_hint,
            ..
        } => SafetyInput {
            x: *x,
            y: *y,
            selector: None,
            app_hint: app_hint.clone(),
            window_hint: window_hint.clone(),
            dry_run,
            ..SafetyInput::default()
        },
        ValidatedParams::ComputerCaptureScreen { .. }
        | ValidatedParams::ComputerLocateElement { .. } => SafetyInput {
            dry_run,
            ..SafetyInput::default()
        },
        _ => {
            return Err(computer_use::SafetyViolation {
                code: "unknown_tool".to_owned(),
                message: "not a computer-use tool".to_owned(),
            })
        }
    };
    let checked = config.computer.check_action(&input).await?;
    Ok(checked.warnings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::ToolRegistry;

    async fn db() -> EvidenceDb {
        EvidenceDb::connect_memory().await.expect("memory db")
    }

    fn invocation(tool_id: &str, params: serde_json::Value, dry_run: bool) -> ToolInvocation {
        ToolInvocation {
            tool_id: tool_id.to_owned(),
            task_id: "task-1".to_owned(),
            agent_id: "agent-1".to_owned(),
            params,
            dry_run,
        }
    }

    /// Serializes tests that flip `TALOS_BROWSER_LIVE`: the routing decision
    /// is process-global, so live-path tests must not overlap. Shares the
    /// worker crate guard so env-mutating worker tests join the same lock.
    async fn env_guard() -> tokio::sync::MutexGuard<'static, ()> {
        // Held across awaits: the mutex is async-aware, so no thread blocks.
        crate::browser_worker::ENV_GUARD.lock().await
    }

    #[tokio::test]
    async fn dry_run_produces_no_side_effects() {
        let db = db().await;
        let engine = policy_broker::DefaultPolicyEngine::new();
        let registry = ToolRegistry::with_builtins();
        let dir = tempfile::tempdir().expect("tempdir");
        let config = SandboxConfig {
            root: dir.path().to_path_buf(),
            ..SandboxConfig::default()
        };
        // Reads are low-risk: dry-run succeeds and mutates nothing.
        let outcome = invoke(
            &db,
            &engine,
            &registry,
            &config,
            invocation(FS_READ, serde_json::json!({ "path": "new.txt" }), true),
        )
        .await
        .expect("dry run");
        assert_eq!(outcome.receipt.status, ToolStatus::Success);
        assert!(outcome.receipt.dry_run);
        assert!(!dir.path().join("new.txt").exists());
        assert!(outcome.receipt.evidence_id.is_some());
        // Writes on a fresh task are denied until evidence exists (gating
        // holds even for dry-run: no silent pass). The read receipt above
        // used task-1, so a *new* task still gates.
        let gated = invoke(
            &db,
            &engine,
            &registry,
            &config,
            ToolInvocation {
                tool_id: FS_WRITE.to_owned(),
                task_id: "task-fresh".to_owned(),
                agent_id: "agent-1".to_owned(),
                params: serde_json::json!({ "path": "new.txt", "content": "hi" }),
                dry_run: true,
            },
        )
        .await
        .expect("dry run");
        assert_eq!(gated.receipt.status, ToolStatus::Denied);
        assert!(!dir.path().join("new.txt").exists());
    }

    #[tokio::test]
    async fn denied_invocations_produce_structured_errors() {
        let db = db().await;
        let engine = policy_broker::DefaultPolicyEngine::new();
        let registry = ToolRegistry::with_builtins();
        let dir = tempfile::tempdir().expect("tempdir");
        let config = SandboxConfig {
            root: dir.path().to_path_buf(),
            ..SandboxConfig::default()
        };
        // process.exec live is disabled by default.
        let outcome = invoke(
            &db,
            &engine,
            &registry,
            &config,
            invocation(
                PROCESS_EXEC,
                serde_json::json!({ "command": "echo", "args": [] }),
                false,
            ),
        )
        .await
        .expect("invoke");
        assert_eq!(outcome.receipt.status, ToolStatus::Disabled);
        let error = outcome.receipt.error.expect("error");
        assert_eq!(error.code, "tool_disabled");
        // Unknown tool is a transport-level error (not a receipt).
        let unknown = invoke(
            &db,
            &engine,
            &registry,
            &config,
            invocation("nope", serde_json::json!({}), true),
        )
        .await;
        assert!(unknown.is_err());
    }

    #[tokio::test]
    async fn receipts_are_recorded() {
        let db = db().await;
        let engine = policy_broker::DefaultPolicyEngine::new();
        let registry = ToolRegistry::with_builtins();
        let dir = tempfile::tempdir().expect("tempdir");
        tokio::fs::write(dir.path().join("a.txt"), b"hello")
            .await
            .expect("seed");
        let config = SandboxConfig {
            root: dir.path().to_path_buf(),
            ..SandboxConfig::default()
        };
        let outcome = invoke(
            &db,
            &engine,
            &registry,
            &config,
            invocation(FS_READ, serde_json::json!({ "path": "a.txt" }), false),
        )
        .await
        .expect("invoke");
        assert_eq!(outcome.receipt.status, ToolStatus::Success);
        let evidence_id = outcome.receipt.evidence_id.expect("evidence");
        let rows = evidence_store::list_recent_evidence(&db, 10)
            .await
            .expect("list");
        assert!(rows.iter().any(|row| row.id == evidence_id));
    }

    #[tokio::test]
    async fn write_requires_evidence_then_compensates() {
        let db = db().await;
        let engine = policy_broker::DefaultPolicyEngine::new();
        let registry = ToolRegistry::with_builtins();
        let dir = tempfile::tempdir().expect("tempdir");
        let config = SandboxConfig {
            root: dir.path().to_path_buf(),
            ..SandboxConfig::default()
        };
        // Seed evidence via a read on the same task first.
        tokio::fs::write(dir.path().join("seed.txt"), b"x")
            .await
            .expect("seed");
        invoke(
            &db,
            &engine,
            &registry,
            &config,
            invocation(FS_READ, serde_json::json!({ "path": "seed.txt" }), false),
        )
        .await
        .expect("read");
        // Live write now passes policy and creates a compensation plan.
        let outcome = invoke(
            &db,
            &engine,
            &registry,
            &config,
            invocation(
                FS_WRITE,
                serde_json::json!({ "path": "out.txt", "content": "hello" }),
                false,
            ),
        )
        .await
        .expect("write");
        assert_eq!(outcome.receipt.status, ToolStatus::Success);
        assert!(dir.path().join("out.txt").exists());
        let plan_id = outcome.receipt.compensation_plan_id.expect("plan");
        assert!(plan_id.starts_with("plan-"));
    }

    fn browser_config(dir: &tempfile::TempDir) -> SandboxConfig {
        SandboxConfig {
            root: dir.path().to_path_buf(),
            ..SandboxConfig::default()
        }
    }

    #[tokio::test]
    async fn browser_dry_run_resolves_without_mutation() {
        let db = db().await;
        let engine = policy_broker::DefaultPolicyEngine::new();
        let registry = ToolRegistry::with_builtins();
        let dir = tempfile::tempdir().expect("tempdir");
        let config = browser_config(&dir);
        let outcome = invoke(
            &db,
            &engine,
            &registry,
            &config,
            invocation(
                BROWSER_CLICK,
                serde_json::json!({ "url": "https://example.com", "selector": "button#ok" }),
                true,
            ),
        )
        .await
        .expect("dry run");
        assert_eq!(outcome.receipt.status, ToolStatus::Denied);
        // Fresh task: mutation needs evidence first — gating holds for dry-run.
        assert!(!dir.path().join("browser-profiles").exists());
    }

    #[tokio::test]
    async fn browser_snapshot_is_read_only_and_evidence_backed() {
        let _guard = env_guard().await;
        let db = db().await;
        let engine = policy_broker::DefaultPolicyEngine::new();
        let registry = ToolRegistry::with_builtins();
        let dir = tempfile::tempdir().expect("tempdir");
        let config = browser_config(&dir);
        let outcome = invoke(
            &db,
            &engine,
            &registry,
            &config,
            invocation(
                BROWSER_SNAPSHOT,
                serde_json::json!({ "url": "https://example.com" }),
                false,
            ),
        )
        .await
        .expect("snapshot");
        assert_eq!(outcome.receipt.status, ToolStatus::Success);
        let dom_hash = outcome.receipt.output["domHash"].as_str().unwrap_or("");
        assert!(dom_hash.starts_with("sha256:"));
        assert!(outcome.receipt.evidence_id.is_some());
    }

    #[tokio::test]
    async fn browser_mutation_needs_evidence_then_succeeds() {
        let _guard = env_guard().await;
        let db = db().await;
        let engine = policy_broker::DefaultPolicyEngine::new();
        let registry = ToolRegistry::with_builtins();
        let dir = tempfile::tempdir().expect("tempdir");
        let config = browser_config(&dir);
        // Seed evidence with a read-only snapshot on the same task.
        invoke(
            &db,
            &engine,
            &registry,
            &config,
            ToolInvocation {
                tool_id: BROWSER_SNAPSHOT.to_owned(),
                task_id: "task-browser".to_owned(),
                agent_id: "agent-1".to_owned(),
                params: serde_json::json!({ "url": "https://example.com" }),
                dry_run: false,
            },
        )
        .await
        .expect("seed");
        let outcome = invoke(
            &db,
            &engine,
            &registry,
            &config,
            ToolInvocation {
                tool_id: BROWSER_CLICK.to_owned(),
                task_id: "task-browser".to_owned(),
                agent_id: "agent-1".to_owned(),
                params: serde_json::json!({
                    "url": "https://example.com",
                    "selector": "button#ok"
                }),
                dry_run: false,
            },
        )
        .await
        .expect("click");
        assert_eq!(outcome.receipt.status, ToolStatus::Success);
        assert!(dir.path().join("browser-profiles").exists());
    }

    #[tokio::test]
    async fn browser_high_risk_requires_human_approval() {
        let _guard = env_guard().await;
        let db = db().await;
        let engine = policy_broker::DefaultPolicyEngine::new();
        let registry = ToolRegistry::with_builtins();
        let dir = tempfile::tempdir().expect("tempdir");
        let config = browser_config(&dir);
        // Seed evidence so the only gate left is the high-risk flag.
        invoke(
            &db,
            &engine,
            &registry,
            &config,
            ToolInvocation {
                tool_id: BROWSER_SNAPSHOT.to_owned(),
                task_id: "task-risk".to_owned(),
                agent_id: "agent-1".to_owned(),
                params: serde_json::json!({ "url": "https://example.com" }),
                dry_run: false,
            },
        )
        .await
        .expect("seed");
        let outcome = invoke(
            &db,
            &engine,
            &registry,
            &config,
            ToolInvocation {
                tool_id: BROWSER_CLICK.to_owned(),
                task_id: "task-risk".to_owned(),
                agent_id: "agent-1".to_owned(),
                params: serde_json::json!({
                    "url": "https://example.com",
                    "selector": "button#buy",
                    "highRisk": true,
                    "highRiskKind": "purchase"
                }),
                dry_run: false,
            },
        )
        .await
        .expect("click");
        assert_eq!(outcome.receipt.status, ToolStatus::Denied);
        let error = outcome.receipt.error.expect("structured error");
        assert_eq!(error.code, "policy_denied");
        assert!(outcome.receipt.policy_reason.contains("human review"));
    }

    #[tokio::test]
    async fn browser_receipts_carry_hashes_and_policy_reference() {
        let _guard = env_guard().await;
        let db = db().await;
        let engine = policy_broker::DefaultPolicyEngine::new();
        let registry = ToolRegistry::with_builtins();
        let dir = tempfile::tempdir().expect("tempdir");
        let config = browser_config(&dir);
        let outcome = invoke(
            &db,
            &engine,
            &registry,
            &config,
            invocation(
                BROWSER_SCREENSHOT,
                serde_json::json!({ "url": "https://example.com" }),
                false,
            ),
        )
        .await
        .expect("screenshot");
        assert_eq!(outcome.receipt.status, ToolStatus::Success);
        assert!(outcome.receipt.output["screenshotHash"]
            .as_str()
            .unwrap_or("")
            .starts_with("sha256:"));
        assert!(!outcome.receipt.policy_reason.is_empty());
        assert!(outcome.receipt.evidence_id.is_some());
    }

    #[tokio::test]
    async fn browser_session_tool_plans_and_runs_mock() {
        let _guard = env_guard().await;
        let db = db().await;
        let engine = policy_broker::DefaultPolicyEngine::new();
        let registry = ToolRegistry::with_builtins();
        let dir = tempfile::tempdir().expect("tempdir");
        let config = browser_config(&dir);
        let planned = invoke(
            &db,
            &engine,
            &registry,
            &config,
            invocation(
                BROWSER_SESSION,
                serde_json::json!({ "operation": "status" }),
                true,
            ),
        )
        .await
        .expect("session dry run");
        assert_eq!(planned.receipt.status, ToolStatus::Success);
        assert!(planned.receipt.dry_run);
        let live = invoke(
            &db,
            &engine,
            &registry,
            &config,
            invocation(
                BROWSER_SESSION,
                serde_json::json!({ "operation": "status" }),
                false,
            ),
        )
        .await
        .expect("session status");
        assert_eq!(live.receipt.status, ToolStatus::Success);
        assert!(live.receipt.output.get("simulated").is_some());
        assert!(live.receipt.evidence_id.is_some());
    }

    #[tokio::test]
    async fn browser_live_worker_crash_is_structured() {
        let _guard = env_guard().await;
        std::env::set_var("TALOS_BROWSER_LIVE", "1");
        std::env::set_var("TALOS_BROWSER_WORKER", "/nonexistent/worker-stub.js");
        let db = db().await;
        let engine = policy_broker::DefaultPolicyEngine::new();
        let registry = ToolRegistry::with_builtins();
        let dir = tempfile::tempdir().expect("tempdir");
        let config = browser_config(&dir);
        let outcome = invoke(
            &db,
            &engine,
            &registry,
            &config,
            invocation(
                BROWSER_SESSION,
                serde_json::json!({ "operation": "status" }),
                false,
            ),
        )
        .await
        .expect("session live");
        std::env::remove_var("TALOS_BROWSER_LIVE");
        std::env::remove_var("TALOS_BROWSER_WORKER");
        // No worker binary: structured failure, never a panic, never a hang.
        assert_eq!(outcome.receipt.status, ToolStatus::Failed);
        let error = outcome.receipt.error.expect("structured error");
        assert!(
            error.code == "browser_worker_crashed"
                || error.code == "worker_spawn_failed"
                || error.code == "worker_crashed",
            "unexpected code {}",
            error.code
        );
        assert!(outcome.receipt.evidence_id.is_some());
    }

    fn computer_config(dir: &tempfile::TempDir, enabled: bool) -> SandboxConfig {
        let config = SandboxConfig {
            root: dir.path().to_path_buf(),
            ..SandboxConfig::default()
        };
        config.computer.set_enabled(enabled);
        config
    }

    #[tokio::test]
    async fn computer_dry_run_mutates_nothing() {
        let db = db().await;
        let engine = policy_broker::DefaultPolicyEngine::new();
        let registry = ToolRegistry::with_builtins();
        let dir = tempfile::tempdir().expect("tempdir");
        // Computer tools stay disabled: dry-run still plans with no side effects.
        let config = computer_config(&dir, false);
        let outcome = invoke(
            &db,
            &engine,
            &registry,
            &config,
            invocation(COMPUTER_CAPTURE_SCREEN, serde_json::json!({}), true),
        )
        .await
        .expect("dry run");
        assert_eq!(outcome.receipt.status, ToolStatus::Success);
        assert!(outcome.receipt.dry_run);
        assert!(!dir.path().join("computer-profiles").exists());
        assert!(outcome.receipt.evidence_id.is_some());
    }

    #[tokio::test]
    async fn computer_safety_envelope_blocks_out_of_bounds() {
        let db = db().await;
        let engine = policy_broker::DefaultPolicyEngine::new();
        let registry = ToolRegistry::with_builtins();
        let dir = tempfile::tempdir().expect("tempdir");
        let config = computer_config(&dir, false);
        let denied = invoke(
            &db,
            &engine,
            &registry,
            &config,
            invocation(
                COMPUTER_MOVE_MOUSE,
                serde_json::json!({ "x": 9999, "y": 10 }),
                true,
            ),
        )
        .await
        .expect("dry run");
        assert_eq!(denied.receipt.status, ToolStatus::Denied);
        let error = denied.receipt.error.expect("structured error");
        assert_eq!(error.code, "out_of_bounds");
    }

    #[tokio::test]
    async fn computer_dangerous_zones_block_sensitive_apps() {
        let db = db().await;
        let engine = policy_broker::DefaultPolicyEngine::new();
        let registry = ToolRegistry::with_builtins();
        let dir = tempfile::tempdir().expect("tempdir");
        let config = computer_config(&dir, true);
        // Password managers and system settings are hard-blocked.
        for hint in ["1Password", "System Settings", "keychain"] {
            let blocked = invoke(
                &db,
                &engine,
                &registry,
                &config,
                invocation(
                    COMPUTER_CLICK,
                    serde_json::json!({ "x": 10, "y": 10, "appHint": hint }),
                    true,
                ),
            )
            .await
            .expect("dry run");
            assert_eq!(blocked.receipt.status, ToolStatus::Denied);
            assert_eq!(
                blocked.receipt.error.expect("structured error").code,
                "dangerous_zone",
                "hint {hint} must be blocked"
            );
        }
    }

    #[tokio::test]
    async fn computer_terminal_zones_warn_without_blocking() {
        let db = db().await;
        let engine = policy_broker::DefaultPolicyEngine::new();
        let registry = ToolRegistry::with_builtins();
        let dir = tempfile::tempdir().expect("tempdir");
        let config = computer_config(&dir, false);
        // Seed evidence with a read-only capture on the same task first.
        invoke(
            &db,
            &engine,
            &registry,
            &config,
            ToolInvocation {
                tool_id: COMPUTER_CAPTURE_SCREEN.to_owned(),
                task_id: "task-warn".to_owned(),
                agent_id: "agent-1".to_owned(),
                params: serde_json::json!({}),
                dry_run: true,
            },
        )
        .await
        .expect("seed");
        let warned = invoke(
            &db,
            &engine,
            &registry,
            &config,
            ToolInvocation {
                tool_id: COMPUTER_MOVE_MOUSE.to_owned(),
                task_id: "task-warn".to_owned(),
                agent_id: "agent-1".to_owned(),
                params: serde_json::json!({ "x": 10, "y": 10, "appHint": "Terminal" }),
                dry_run: true,
            },
        )
        .await
        .expect("dry run");
        assert_eq!(warned.receipt.status, ToolStatus::Success);
        let warnings = warned.receipt.output["warnings"]
            .as_array()
            .expect("warnings surfaced in output");
        assert!(!warnings.is_empty());
    }

    #[tokio::test]
    async fn computer_emergency_stop_blocks_everything() {
        let db = db().await;
        let engine = policy_broker::DefaultPolicyEngine::new();
        let registry = ToolRegistry::with_builtins();
        let dir = tempfile::tempdir().expect("tempdir");
        let config = computer_config(&dir, true);
        config.computer.engage_stop();
        let stopped = invoke(
            &db,
            &engine,
            &registry,
            &config,
            invocation(COMPUTER_CAPTURE_SCREEN, serde_json::json!({}), true),
        )
        .await
        .expect("stopped");
        assert_eq!(stopped.receipt.status, ToolStatus::Denied);
        assert_eq!(
            stopped.receipt.error.expect("structured error").code,
            "emergency_stop"
        );
        config.computer.clear_stop();
        let resumed = invoke(
            &db,
            &engine,
            &registry,
            &config,
            invocation(COMPUTER_CAPTURE_SCREEN, serde_json::json!({}), true),
        )
        .await
        .expect("resumed");
        assert_eq!(resumed.receipt.status, ToolStatus::Success);
    }

    #[tokio::test]
    async fn computer_mutation_needs_evidence_then_succeeds() {
        let db = db().await;
        let engine = policy_broker::DefaultPolicyEngine::new();
        let registry = ToolRegistry::with_builtins();
        let mut registry = registry;
        registry
            .set_enabled(COMPUTER_CAPTURE_SCREEN, true)
            .expect("enable");
        registry
            .set_enabled(COMPUTER_MOVE_MOUSE, true)
            .expect("enable");
        let dir = tempfile::tempdir().expect("tempdir");
        let config = computer_config(&dir, true);
        // Fresh task: medium-risk mutation is denied until evidence exists.
        let gated = invoke(
            &db,
            &engine,
            &registry,
            &config,
            ToolInvocation {
                tool_id: COMPUTER_MOVE_MOUSE.to_owned(),
                task_id: "task-cu-fresh".to_owned(),
                agent_id: "agent-1".to_owned(),
                params: serde_json::json!({ "x": 100, "y": 200 }),
                dry_run: false,
            },
        )
        .await
        .expect("gated");
        assert_eq!(gated.receipt.status, ToolStatus::Denied);
        // Seed evidence with a read-only capture on the same task, then retry.
        invoke(
            &db,
            &engine,
            &registry,
            &config,
            ToolInvocation {
                tool_id: COMPUTER_CAPTURE_SCREEN.to_owned(),
                task_id: "task-cu".to_owned(),
                agent_id: "agent-1".to_owned(),
                params: serde_json::json!({}),
                dry_run: false,
            },
        )
        .await
        .expect("seed");
        let done = invoke(
            &db,
            &engine,
            &registry,
            &config,
            ToolInvocation {
                tool_id: COMPUTER_MOVE_MOUSE.to_owned(),
                task_id: "task-cu".to_owned(),
                agent_id: "agent-1".to_owned(),
                params: serde_json::json!({ "x": 100, "y": 200 }),
                dry_run: false,
            },
        )
        .await
        .expect("move");
        assert_eq!(done.receipt.status, ToolStatus::Success);
        assert!(done.receipt.output["screenHash"]
            .as_str()
            .unwrap_or("")
            .starts_with("sha256:"));
    }

    #[tokio::test]
    async fn computer_high_risk_requires_human_approval() {
        let db = db().await;
        let engine = policy_broker::DefaultPolicyEngine::new();
        let registry = ToolRegistry::with_builtins();
        let mut registry = registry;
        registry.set_enabled(COMPUTER_CLICK, true).expect("enable");
        let dir = tempfile::tempdir().expect("tempdir");
        let config = computer_config(&dir, true);
        let outcome = invoke(
            &db,
            &engine,
            &registry,
            &config,
            ToolInvocation {
                tool_id: COMPUTER_CLICK.to_owned(),
                task_id: "task-risk".to_owned(),
                agent_id: "agent-1".to_owned(),
                params: serde_json::json!({
                    "selector": "button#buy",
                    "highRisk": true,
                    "highRiskKind": "purchase"
                }),
                dry_run: false,
            },
        )
        .await
        .expect("click");
        assert_eq!(outcome.receipt.status, ToolStatus::Denied);
        let error = outcome.receipt.error.expect("structured error");
        assert_eq!(error.code, "policy_denied");
        assert!(outcome.receipt.policy_reason.contains("human review"));
    }

    #[tokio::test]
    async fn computer_receipts_carry_hashes_and_policy_reference() {
        let db = db().await;
        let engine = policy_broker::DefaultPolicyEngine::new();
        let registry = ToolRegistry::with_builtins();
        let dir = tempfile::tempdir().expect("tempdir");
        let config = computer_config(&dir, false);
        let outcome = invoke(
            &db,
            &engine,
            &registry,
            &config,
            invocation(
                COMPUTER_LOCATE_ELEMENT,
                serde_json::json!({ "selector": "button#ok" }),
                true,
            ),
        )
        .await
        .expect("locate");
        assert_eq!(outcome.receipt.status, ToolStatus::Success);
        assert!(outcome.receipt.output["boundingBox"].is_object());
        assert!(!outcome.receipt.policy_reason.is_empty());
        assert!(outcome.receipt.evidence_id.is_some());
    }

    #[tokio::test]
    async fn computer_low_confidence_is_denied() {
        let db = db().await;
        let engine = policy_broker::DefaultPolicyEngine::new();
        let registry = ToolRegistry::with_builtins();
        let mut registry = registry;
        registry
            .set_enabled(COMPUTER_LOCATE_ELEMENT, true)
            .expect("enable");
        let dir = tempfile::tempdir().expect("tempdir");
        let config = computer_config(&dir, false);
        let outcome = invoke(
            &db,
            &engine,
            &registry,
            &config,
            invocation(
                COMPUTER_LOCATE_ELEMENT,
                serde_json::json!({ "selector": "button#uncertain-ok" }),
                true,
            ),
        )
        .await
        .expect("locate");
        assert_eq!(outcome.receipt.status, ToolStatus::Denied);
        assert_eq!(
            outcome.receipt.error.expect("structured error").code,
            "low_confidence"
        );
    }
}
