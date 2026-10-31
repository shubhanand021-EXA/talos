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
    // High-risk browser mutations (form submit, purchase, settings, upload,
    // destructive) escalate to human review via an explicit approval flag.
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
    let (status, output, error, _) =
        run_adapter(mode, config, &invocation.task_id, &manifest, &validated).await?;
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
) -> Result<(
    ToolStatus,
    serde_json::Value,
    Option<ToolErrorDetail>,
    Option<String>,
)> {
    use crate::tools_browser as browser;
    use crate::tools_exec as exec;
    use crate::tools_fs as fs;
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
        (crate::model::BROWSER_NAVIGATE, ValidatedParams::BrowserNavigate { url, timeout_ms }) => {
            if mode == ExecutionMode::DryRun {
                return Ok((
                    ToolStatus::Success,
                    browser::navigate_plan(url, *timeout_ms),
                    None,
                    None,
                ));
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
            },
        ) => {
            // Read-only: dry-run and live share the deterministic rendering.
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
            ValidatedParams::BrowserScreenshot { url, full_page },
        ) => {
            // Read-only hash: dry-run and live share the deterministic rendering.
            let _ = mode;
            Ok((
                ToolStatus::Success,
                browser::screenshot_live(url, *full_page),
                None,
                None,
            ))
        }
        (crate::model::BROWSER_SNAPSHOT, ValidatedParams::BrowserSnapshot { url, selector }) => {
            // Read-only: dry-run and live share the deterministic rendering.
            let _ = mode;
            Ok((
                ToolStatus::Success,
                browser::snapshot_live(url, selector.as_deref()),
                None,
                None,
            ))
        }
        _ => Ok((
            ToolStatus::Failed,
            serde_json::json!({}),
            Some(err("invalid_params", "params do not match tool".to_owned())),
            None,
        )),
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
}
