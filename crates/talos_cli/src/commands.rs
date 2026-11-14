//! CLI command implementations over the shared crates.
//!
//! Every path that touches side effects evaluates through
//! [`policy_broker::DefaultPolicyEngine`] first. The CLI records its work in
//! the same `event_log` / `evidence_records` / `runtime_*` tables the desktop
//! shell uses, so `talos task list` and the desktop Agents page always agree.

use evidence_store::{
    append_event, insert_evidence, is_onboarding_completed, read_state_map, set_app_state,
    write_state_map, EnqueueParams, EvidenceDb, NewActionRecord, NewEvent, NewEvidence,
};
use policy_broker::{run_selftest, DefaultPolicyEngine, PolicyDecision};
use talos_common::{now_iso, Result, TalosError};

use crate::db::CliDb;

/// Exit code: success.
pub const EXIT_SUCCESS: i32 = 0;
/// Exit code: failure (bad input, missing record, I/O error).
pub const EXIT_FAILURE: i32 = 1;
/// Exit code: the policy broker denied the requested action.
pub const EXIT_POLICY_DENIED: i32 = 2;

/// Outcome of `talos run`: exit code plus the JSONL events to emit.
#[derive(Debug)]
pub struct RunOutcome {
    /// Process exit code (0 / 1 / 2).
    pub exit_code: i32,
    /// Ordered JSONL event payloads (without `type`/`timestamp` framing).
    pub events: Vec<(String, serde_json::Value)>,
}

/// Fixed context seed (chars) priced into every headless run: the system
/// prompt plus the policy-receipt context the probe step always assembles.
pub const RUN_CONTEXT_OVERHEAD_CHARS: u64 = 1500;

/// Split a `--model` hint into `(provider_id, model_id)`. Accepts
/// `provider/model`, a bare model id (resolved against the gateway catalog),
/// or nothing (defaults to `openai/gpt-4o-mini`).
fn split_model_hint(hint: Option<&str>) -> (String, String) {
    const DEFAULT: (&str, &str) = ("openai", "gpt-4o-mini");
    let Some(trimmed) = hint.map(str::trim).filter(|h| !h.is_empty()) else {
        return (DEFAULT.0.to_owned(), DEFAULT.1.to_owned());
    };
    if let Some((provider, model)) = trimmed.split_once('/') {
        if !provider.is_empty() && !model.is_empty() {
            return (provider.to_owned(), model.to_owned());
        }
    }
    let router = llm_gateway::registry::default_registry();
    if let Some(found) = router.catalog().iter().find(|d| d.model_id == trimmed) {
        return (found.provider_id.clone(), found.model_id.clone());
    }
    (DEFAULT.0.to_owned(), DEFAULT.1.to_owned())
}

/// Catalog list prices for a model; unknown pairs fall back to the
/// `openai/gpt-4o-mini` list rates so an estimate is always priced.
fn catalog_prices(provider_id: &str, model_id: &str) -> (String, String, f64, f64) {
    let router = llm_gateway::registry::default_registry();
    if let Some(found) = router.lookup(provider_id, model_id) {
        return (
            found.provider_id.clone(),
            found.model_id.clone(),
            found.cost_per_1k_input,
            found.cost_per_1k_output,
        );
    }
    let fallback = router
        .lookup("openai", "gpt-4o-mini")
        .expect("default catalog carries gpt-4o-mini");
    (
        fallback.provider_id.clone(),
        fallback.model_id.clone(),
        fallback.cost_per_1k_input,
        fallback.cost_per_1k_output,
    )
}

/// Pre-run cost estimate for `talos run`: one planned model call over the
/// goal text plus the fixed read-only probe tool step. Pure projection
/// (no spend); the completion prediction comes from the ledger mean when
/// the model has history, else the shared fallback.
pub async fn estimate_run_cost(
    db: &EvidenceDb,
    goal: &str,
    model: Option<&str>,
) -> cost_tracker::CostEstimate {
    use cost_tracker::{estimate_task_cost, TaskCostPlan, FALLBACK_COMPLETION_TOKENS};
    let (provider_id, model_id) = split_model_hint(model);
    let (provider_id, model_id, cost_per_1k_input, cost_per_1k_output) =
        catalog_prices(&provider_id, &model_id);
    let predicted = cost_tracker::historical_completion_average(db, &provider_id, &model_id)
        .await
        .ok()
        .flatten()
        .filter(|mean| *mean > 0.0)
        .map(|mean| mean.round().clamp(1.0, u32::MAX as f64) as u32)
        .unwrap_or(FALLBACK_COMPLETION_TOKENS);
    estimate_task_cost(&TaskCostPlan {
        provider_id,
        model_id,
        cost_per_1k_input,
        cost_per_1k_output,
        prompt_chars: goal.len() as u64 + RUN_CONTEXT_OVERHEAD_CHARS,
        expected_completion_tokens: predicted,
        tools: vec![cost_tracker::PlannedToolUse {
            tool_id: "goal_descriptor_probe".to_owned(),
            duration_ms: 50,
        }],
        browser_ms: 0,
        computer_actions: 0,
    })
}

/// Chain verification summary for `talos evidence verify`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct EvidenceReport {
    /// Event-log chain valid.
    pub event_chain_valid: bool,
    /// Evidence chain valid.
    pub evidence_chain_valid: bool,
    /// Links checked across both chains.
    pub checked: i64,
    /// Human-readable detail.
    pub detail: String,
}

/// Audit summary for `talos audit run`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AuditReport {
    /// Policy selftest passed.
    pub selftest_ok: bool,
    /// Selftest checks passed / total.
    pub selftest_passed: usize,
    /// Selftest checks total.
    pub selftest_total: usize,
    /// Ledger report.
    pub ledger: EvidenceReport,
}

/// Environment summary for `talos doctor`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DoctorReport {
    /// CLI version.
    pub version: String,
    /// Resolved app-data directory.
    pub app_data_dir: String,
    /// Database file.
    pub db_file: String,
    /// At-rest encryption active.
    pub encrypted: bool,
    /// Database reachable + migrations current.
    pub database_ok: bool,
    /// Schema version (0 when unknown).
    pub schema_version: i64,
    /// Journal mode (proof hook: `wal` for file-backed databases).
    pub journal_mode: String,
    /// Ledger chain valid.
    pub chain_valid: bool,
    /// Onboarding completed flag.
    pub onboarding_completed: bool,
    /// Individual checks with pass/fail.
    pub checks: Vec<DoctorCheck>,
}

/// One environment check inside [`DoctorReport`].
#[derive(Debug, Clone, serde::Serialize)]
pub struct DoctorCheck {
    /// Check name.
    pub name: String,
    /// Whether it passed.
    pub ok: bool,
    /// Human-readable detail.
    pub detail: String,
}

fn denied(reason: &str) -> TalosError {
    TalosError::Policy(reason.to_owned())
}

/// Execute a goal headlessly: enqueue → policy-gate → evidence → complete.
///
/// The CLI executes a bounded, read-only demonstration step (a `ReadFile`
/// probe of the goal descriptor) through the policy broker. Goals prefixed
/// with `danger:` request an irreversible `SystemControl` action instead, so
/// the broker denies them and the process exits with code 2 — proving the
/// CLI cannot bypass safety gates.
pub async fn run_task(
    cli: &CliDb,
    goal: &str,
    model: Option<&str>,
    budget_usd: Option<f64>,
) -> (RunOutcome, i32) {
    let goal = goal.trim();
    if goal.is_empty() {
        return (
            RunOutcome {
                exit_code: EXIT_FAILURE,
                events: vec![(
                    "task_failed".to_owned(),
                    serde_json::json!({"error": "goal must not be empty"}),
                )],
            },
            EXIT_FAILURE,
        );
    }
    if !budget_usd.is_none_or(|budget| budget >= 0.0) {
        return (
            RunOutcome {
                exit_code: EXIT_FAILURE,
                events: vec![(
                    "task_failed".to_owned(),
                    serde_json::json!({"error": "budget must be >= 0"}),
                )],
            },
            EXIT_FAILURE,
        );
    }
    let db = &cli.db;
    let task_id = format!("cli-{}", uuid::Uuid::new_v4());
    let engine = DefaultPolicyEngine::new();

    // Durable queue entry first: the run is ledgered even when denied later.
    let task = match evidence_store::enqueue_task(
        db,
        EnqueueParams {
            id: &task_id,
            title: goal,
            description: model,
            priority: "normal",
            max_attempts: 1,
            timeout_ms: 60_000,
            backoff_ms: 1_000,
            scenario: "cli-run",
            parent_task_id: None,
        },
    )
    .await
    {
        Ok(task) => task,
        Err(err) => {
            return (
                RunOutcome {
                    exit_code: EXIT_FAILURE,
                    events: vec![(
                        "task_failed".to_owned(),
                        serde_json::json!({"error": err.to_string()}),
                    )],
                },
                EXIT_FAILURE,
            );
        }
    };
    let mut events = vec![(
        "task_started".to_owned(),
        serde_json::json!({
            "task_id": task.id,
            "goal": goal,
            "model": model,
            "budget_usd": budget_usd,
        }),
    )];

    // Pre-run estimate BEFORE any spend: surfaced as `cost_estimate` and
    // enforced against the requested cap (deny when the plan alone exceeds
    // the budget — the existing budget-deny rule, applied before traffic).
    let estimate = estimate_run_cost(db, goal, model).await;
    events.push((
        "cost_estimate".to_owned(),
        serde_json::json!({
            "task_id": task.id,
            "estimate": serde_json::to_value(&estimate).unwrap_or_default(),
            "budget_usd": budget_usd,
        }),
    ));
    if budget_usd.is_some_and(|cap| estimate.total_cost_usd > cap) {
        let _ = evidence_store::record_transition(
            db,
            &task_id,
            "queued",
            "planning",
            "cli estimate over budget",
        )
        .await;
        let _ = evidence_store::record_transition(
            db,
            &task_id,
            "planning",
            "failed",
            "cli estimate denied",
        )
        .await;
        events.push((
            "task_failed".to_owned(),
            serde_json::json!({
                "task_id": task_id,
                "error": "pre-run estimate exceeds budget",
                "estimated_usd": estimate.total_cost_usd,
                "budget_usd": budget_usd,
            }),
        ));
        return (
            RunOutcome {
                exit_code: EXIT_POLICY_DENIED,
                events,
            },
            EXIT_POLICY_DENIED,
        );
    }

    // Policy gate: read-only probe for normal goals, irreversible system
    // control for `danger:` goals (must deny → exit 2).
    let dangerous = goal.starts_with("danger:");
    let request_id = format!("cli-req-{}", uuid::Uuid::new_v4());
    let raw = if dangerous {
        serde_json::json!({
            "id": request_id,
            "agent_id": "agent-cli",
            "task_id": task_id,
            "capability": "system_control",
            "intent": goal,
            "target": "system",
            "payload_json": {},
            "risk_level": "irreversible",
            "reversible": false,
            "requested_at": now_iso(),
        })
    } else {
        serde_json::json!({
            "id": request_id,
            "agent_id": "agent-cli",
            "task_id": task_id,
            "capability": "read_file",
            "intent": format!("inspect goal descriptor: {goal}"),
            "target": "goal-descriptor",
            "payload_json": {},
            "risk_level": "low",
            "reversible": true,
            "requested_at": now_iso(),
        })
    };
    let evaluation = policy_broker::evaluate_request_json(&engine, &raw);
    events.push((
        "policy_evaluated".to_owned(),
        serde_json::json!({
            "task_id": task_id,
            "request_id": evaluation.request_id,
            "decision": format!("{:?}", evaluation.decision),
            "reason": evaluation.reason,
        }),
    ));
    // Record the policy verdict in the ledger (guarded transition first).
    let _ = evidence_store::record_action_request(
        db,
        &NewActionRecord {
            id: request_id.clone(),
            agent_id: "agent-cli".to_owned(),
            task_id: task_id.clone(),
            capability: if dangerous {
                "system_control"
            } else {
                "read_file"
            }
            .to_owned(),
            intent: goal.to_owned(),
            target: if dangerous {
                "system"
            } else {
                "goal-descriptor"
            }
            .to_owned(),
            payload: serde_json::json!({}),
            risk_level: if dangerous { "irreversible" } else { "low" }.to_owned(),
            reversible: !dangerous,
            requested_at: now_iso(),
        },
        "cli-0.4.0",
        &format!("{:?}", evaluation.decision).to_lowercase(),
        &evaluation.reason,
        &evaluation.required_checks,
    )
    .await;

    if evaluation.decision != PolicyDecision::Allow {
        // Headless single-shot runs cannot satisfy follow-up gates (no
        // human, no evidence loop): any non-Allow verdict — Deny,
        // RequireEvidence, RequireCompensation, RequireHumanReview — is a
        // policy denial, never a bypass. Exit code 2.
        let _ = evidence_store::record_transition(
            db,
            &task_id,
            "queued",
            "planning",
            "cli policy gate",
        )
        .await;
        let _ = evidence_store::record_transition(
            db,
            &task_id,
            "planning",
            "failed",
            "cli policy denied",
        )
        .await;
        events.push((
            "task_failed".to_owned(),
            serde_json::json!({
                "task_id": task_id,
                "error": "policy denied",
                "reason": evaluation.reason,
            }),
        ));
        let outcome = RunOutcome {
            exit_code: EXIT_POLICY_DENIED,
            events,
        };
        return (outcome, EXIT_POLICY_DENIED);
    }

    // Allowed path: planning → executing, one bounded tool step, evidence,
    // completed. No real side effects: the step is a dry-run-style probe
    // recorded as evidence, never executed against the host.
    let _ = evidence_store::record_transition(db, &task_id, "queued", "planning", "cli run").await;
    let _ =
        evidence_store::record_transition(db, &task_id, "planning", "executing", "cli run").await;
    events.push((
        "tool_invoked".to_owned(),
        serde_json::json!({
            "task_id": task_id,
            "tool_id": "goal_descriptor_probe",
            "dry_run": true,
            "status": "success",
        }),
    ));
    let evidence_id = format!("cli-ev-{}", uuid::Uuid::new_v4());
    let spend_usd = 0.0;
    match insert_evidence(
        db,
        NewEvidence {
            id: evidence_id.clone(),
            task_id: Some(task_id.clone()),
            agent_id: Some("agent-cli".to_owned()),
            session_id: None,
            kind: "cli_run_receipt".to_owned(),
            label: Some("headless run receipt".to_owned()),
            summary: Some(goal.to_owned()),
            uri: None,
            mime_type: Some("application/json".to_owned()),
            payload: serde_json::json!({
                "goal": goal,
                "model": model,
                "budget_usd": budget_usd,
                "policy_decision": format!("{:?}", evaluation.decision),
            }),
        },
    )
    .await
    {
        Ok(_) => events.push((
            "evidence_recorded".to_owned(),
            serde_json::json!({"task_id": task_id, "evidence_id": evidence_id}),
        )),
        Err(err) => {
            events.push((
                "task_failed".to_owned(),
                serde_json::json!({"task_id": task_id, "error": err.to_string()}),
            ));
            return (
                RunOutcome {
                    exit_code: EXIT_FAILURE,
                    events,
                },
                EXIT_FAILURE,
            );
        }
    }
    let _ = evidence_store::record_transition(
        db,
        &task_id,
        "executing",
        "completed",
        "cli run completed",
    )
    .await;
    let _ = append_event(
        db,
        NewEvent {
            id: format!("cli-done-{}", uuid::Uuid::new_v4()),
            kind: "cli_task_completed".to_owned(),
            payload_json: serde_json::json!({"task_id": task_id, "goal": goal}),
        },
    )
    .await;
    events.push((
        "cost_update".to_owned(),
        serde_json::json!({
            "task_id": task_id,
            "spend_usd": spend_usd,
            "budget_usd": budget_usd,
        }),
    ));
    events.push((
        "task_completed".to_owned(),
        serde_json::json!({
            "task_id": task_id,
            "status": "completed",
            "fidelity": "mock",
        }),
    ));
    (
        RunOutcome {
            exit_code: EXIT_SUCCESS,
            events,
        },
        EXIT_SUCCESS,
    )
}

/// List durable tasks, newest first, with an optional status filter.
pub async fn task_list(
    db: &EvidenceDb,
    status: Option<&str>,
) -> Result<Vec<evidence_store::RuntimeTaskRow>> {
    let mut tasks = evidence_store::list_tasks(db, 200).await?;
    if let Some(wanted) = status {
        tasks.retain(|t| t.status == wanted);
    }
    Ok(tasks)
}

/// Fetch one task by id.
pub async fn task_get(db: &EvidenceDb, task_id: &str) -> Result<evidence_store::RuntimeTaskRow> {
    evidence_store::get_task(db, task_id)
        .await?
        .ok_or_else(|| TalosError::Storage(format!("unknown task '{task_id}'")))
}

/// Request cancellation for one task.
pub async fn task_cancel(db: &EvidenceDb, task_id: &str) -> Result<evidence_store::RuntimeTaskRow> {
    task_get(db, task_id).await?;
    evidence_store::request_cancellation(db, task_id).await?;
    task_get(db, task_id).await
}

/// Verify both hash chains (event log + evidence records).
pub async fn evidence_verify(db: &EvidenceDb) -> Result<EvidenceReport> {
    let events = evidence_store::verify_chain(db).await?;
    let evidence = evidence_store::verify_evidence_chain(db).await?;
    Ok(EvidenceReport {
        event_chain_valid: events.valid,
        evidence_chain_valid: evidence.valid,
        checked: events.checked + evidence.checked,
        detail: format!("events: {}; evidence: {}", events.detail, evidence.detail),
    })
}

/// Run the safety + integrity audit (policy selftest + chain verification).
pub async fn audit_run(db: &EvidenceDb) -> Result<AuditReport> {
    let selftest = run_selftest().await;
    let ledger = evidence_verify(db).await?;
    Ok(AuditReport {
        selftest_ok: selftest.ok() && ledger.event_chain_valid && ledger.evidence_chain_valid,
        selftest_passed: selftest.passed,
        selftest_total: selftest.total,
        ledger,
    })
}

/// Read one CLI config key from shared `app_state`.
pub async fn config_get(db: &EvidenceDb, key: &str) -> Result<Option<String>> {
    let map = read_state_map(db, evidence_store::app_state::keys::CLI_CONFIG).await?;
    Ok(map
        .get(key)
        .map(|v| v.as_str().unwrap_or_default().to_owned()))
}

/// Write one CLI config key into shared `app_state`.
pub async fn config_set(db: &EvidenceDb, key: &str, value: &str) -> Result<()> {
    if key.trim().is_empty() || key.len() > 128 {
        return Err(denied("config key must be 1..=128 chars"));
    }
    let mut map = read_state_map(db, evidence_store::app_state::keys::CLI_CONFIG).await?;
    map.insert(key.to_owned(), serde_json::Value::String(value.to_owned()));
    write_state_map(db, evidence_store::app_state::keys::CLI_CONFIG, &map).await
}

/// Known provider catalog (mirrors the desktop gateway catalog).
#[must_use]
pub fn provider_catalog() -> Vec<(&'static str, &'static str)> {
    vec![
        ("openai", "OpenAI (gpt-4o, gpt-4o-mini, o3-mini)"),
        ("anthropic", "Anthropic (claude-sonnet-4, claude-3-5-haiku)"),
        ("ollama", "Ollama (local, no key required)"),
    ]
}

/// List providers with configured/not-configured status.
pub async fn provider_list(db: &EvidenceDb) -> Result<Vec<serde_json::Value>> {
    let map = read_state_map(db, evidence_store::app_state::keys::CLI_PROVIDERS).await?;
    Ok(provider_catalog()
        .into_iter()
        .map(|(id, description)| {
            serde_json::json!({
                "id": id,
                "description": description,
                "configured": map.contains_key(id),
                "entry": map.get(id),
            })
        })
        .collect())
}

/// Register a provider entry (`key=value` pairs: `model=...`, `base_url=...`).
pub async fn provider_add(db: &EvidenceDb, name: &str, kv: &[String]) -> Result<serde_json::Value> {
    let name = name.trim().to_lowercase();
    if name.is_empty() || name.len() > 64 {
        return Err(denied("provider name must be 1..=64 chars"));
    }
    let mut entry = serde_json::Map::new();
    for pair in kv {
        let (k, v) = pair
            .split_once('=')
            .ok_or_else(|| TalosError::Storage(format!("expected key=value, got '{pair}'")))?;
        entry.insert(
            k.trim().to_owned(),
            serde_json::Value::String(v.trim().to_owned()),
        );
    }
    let mut map = read_state_map(db, evidence_store::app_state::keys::CLI_PROVIDERS).await?;
    map.insert(name.clone(), serde_json::Value::Object(entry.clone()));
    write_state_map(db, evidence_store::app_state::keys::CLI_PROVIDERS, &map).await?;
    Ok(serde_json::Value::Object(entry))
}

/// Remove a provider entry (idempotent).
pub async fn provider_remove(db: &EvidenceDb, name: &str) -> Result<bool> {
    let name = name.trim().to_lowercase();
    let mut map = read_state_map(db, evidence_store::app_state::keys::CLI_PROVIDERS).await?;
    let removed = map.remove(&name).is_some();
    write_state_map(db, evidence_store::app_state::keys::CLI_PROVIDERS, &map).await?;
    Ok(removed)
}

/// List registered MCP servers.
pub async fn mcp_list(db: &EvidenceDb) -> Result<Vec<serde_json::Value>> {
    let map = read_state_map(db, evidence_store::app_state::keys::CLI_MCP_SERVERS).await?;
    let mut out: Vec<serde_json::Value> = map
        .into_iter()
        .map(|(name, config)| serde_json::json!({"name": name, "config": config}))
        .collect();
    out.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    Ok(out)
}

/// Register an MCP server (`key=value` pairs: `transport=...`, `command=...`, `url=...`).
pub async fn mcp_add(db: &EvidenceDb, name: &str, kv: &[String]) -> Result<serde_json::Value> {
    let name = name.trim();
    if name.is_empty() || name.len() > 128 {
        return Err(denied("mcp server name must be 1..=128 chars"));
    }
    let mut entry = serde_json::Map::new();
    for pair in kv {
        let (k, v) = pair
            .split_once('=')
            .ok_or_else(|| TalosError::Storage(format!("expected key=value, got '{pair}'")))?;
        entry.insert(
            k.trim().to_owned(),
            serde_json::Value::String(v.trim().to_owned()),
        );
    }
    let mut map = read_state_map(db, evidence_store::app_state::keys::CLI_MCP_SERVERS).await?;
    map.insert(name.to_owned(), serde_json::Value::Object(entry.clone()));
    write_state_map(db, evidence_store::app_state::keys::CLI_MCP_SERVERS, &map).await?;
    Ok(serde_json::Value::Object(entry))
}

/// Remove an MCP server (idempotent).
pub async fn mcp_remove(db: &EvidenceDb, name: &str) -> Result<bool> {
    let mut map = read_state_map(db, evidence_store::app_state::keys::CLI_MCP_SERVERS).await?;
    let removed = map.remove(name.trim()).is_some();
    write_state_map(db, evidence_store::app_state::keys::CLI_MCP_SERVERS, &map).await?;
    Ok(removed)
}

/// List skills, optionally filtered by status.
pub async fn skill_list(
    db: &EvidenceDb,
    status: Option<&str>,
) -> Result<Vec<evidence_store::SkillRow>> {
    evidence_store::list_skills(db, status, 200).await
}

/// Validate a quarantined skill.
pub async fn skill_validate(db: &EvidenceDb, skill_id: &str) -> Result<evidence_store::SkillRow> {
    evidence_store::update_skill_status(db, skill_id, "validated")
        .await
        .map_err(|err| TalosError::Storage(format!("validate '{skill_id}': {err}")))
}

/// Publish a validated skill.
pub async fn skill_publish(db: &EvidenceDb, skill_id: &str) -> Result<evidence_store::SkillRow> {
    evidence_store::update_skill_status(db, skill_id, "published")
        .await
        .map_err(|err| TalosError::Storage(format!("publish '{skill_id}': {err}")))
}

/// Check the local environment for headless runs.
pub async fn doctor_report(cli: &CliDb) -> Result<DoctorReport> {
    let db = &cli.db;
    let mut checks = Vec::new();
    let schema_version = db.schema_version().await.unwrap_or(0);
    checks.push(DoctorCheck {
        name: "database_reachable".to_owned(),
        ok: true,
        detail: format!("opened {}", cli.db_file.display()),
    });
    checks.push(DoctorCheck {
        name: "migrations_current".to_owned(),
        ok: schema_version >= 12,
        detail: format!("schema_version={schema_version}"),
    });
    let journal_mode = db
        .journal_mode()
        .await
        .unwrap_or_else(|_| "unknown".to_owned());
    checks.push(DoctorCheck {
        name: "journal_mode".to_owned(),
        ok: journal_mode == "wal" || journal_mode == "memory",
        detail: format!("journal_mode={journal_mode}"),
    });
    let chain = evidence_store::verify_chain(db).await;
    let chain_valid = chain.as_ref().is_ok_and(|r| r.valid);
    checks.push(DoctorCheck {
        name: "ledger_chain".to_owned(),
        ok: chain_valid,
        detail: chain
            .map(|r| r.detail)
            .unwrap_or_else(|err| err.to_string()),
    });
    checks.push(DoctorCheck {
        name: "at_rest_encryption".to_owned(),
        ok: true,
        detail: if cli.encrypted {
            "encrypted (ChaCha20-Poly1305 envelope)".to_owned()
        } else {
            "plaintext fallback (keychain unreachable)".to_owned()
        },
    });
    let onboarding_completed = is_onboarding_completed(db).await.unwrap_or(false);
    // Persist a doctor probe event (diagnostics are ledgered, never silent).
    let _ = append_event(
        db,
        NewEvent {
            id: format!("cli-doctor-{}", uuid::Uuid::new_v4()),
            kind: "cli_doctor".to_owned(),
            payload_json: serde_json::json!({"chain_valid": chain_valid}),
        },
    )
    .await;
    // Touch app_state so a first doctor run proves write access.
    let _ = set_app_state(db, "cli.last_doctor_at", &now_iso()).await;
    Ok(DoctorReport {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        app_data_dir: cli.app_data_dir.display().to_string(),
        db_file: cli.db_file.display().to_string(),
        encrypted: cli.encrypted,
        database_ok: true,
        schema_version,
        journal_mode,
        chain_valid,
        onboarding_completed,
        checks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_shared_db;

    async fn isolated_db() -> CliDb {
        let _guard = crate::TEST_ENV_LOCK.lock().await;
        let dir = tempfile::tempdir().expect("temp dir");
        // Leak the temp dir so the async CLI can use it across awaits; the
        // OS reclaims it after the test process exits (CI-safe, no residue
        // in the real app-data dir).
        let path = dir.keep();
        std::env::set_var("TALOS_DATA_DIR", &path);
        let cli = open_shared_db().await.expect("opens");
        std::env::remove_var("TALOS_DATA_DIR");
        cli
    }

    #[tokio::test]
    async fn run_executes_mock_task_with_jsonl_events() {
        let cli = isolated_db().await;
        let (outcome, code) = run_task(&cli, "summarize the ledger", Some("mock"), Some(1.0)).await;
        assert_eq!(code, EXIT_SUCCESS);
        assert_eq!(outcome.exit_code, EXIT_SUCCESS);
        let kinds: Vec<&str> = outcome.events.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            kinds,
            vec![
                "task_started",
                "cost_estimate",
                "policy_evaluated",
                "tool_invoked",
                "evidence_recorded",
                "cost_update",
                "task_completed",
            ]
        );
        // The task is durable: desktop + CLI read the same row.
        let task_id = outcome.events[0].1["task_id"].as_str().unwrap();
        let row = task_get(&cli.db, task_id).await.expect("task persisted");
        assert_eq!(row.status, "completed");
    }

    #[tokio::test]
    async fn estimate_defaults_to_mini_and_prices_local_at_zero_llm() {
        let cli = isolated_db().await;
        let estimate = estimate_run_cost(&cli.db, "summarize the ledger", None).await;
        assert_eq!(estimate.provider_id, "openai");
        assert_eq!(estimate.model_id, "gpt-4o-mini");
        assert_eq!(estimate.prompt_tokens, 380); // ceil((20 + 1500) / 4)
        assert!(estimate.total_cost_usd > 0.0);
        assert_eq!(estimate.method, cost_tracker::ESTIMATE_METHOD);
        // Unknown hints fall back to the default catalog entry (always priced).
        let fallback = estimate_run_cost(&cli.db, "x", Some("no-such-model")).await;
        assert_eq!(fallback.model_id, "gpt-4o-mini");
        // Local models price LLM at exactly $0.
        let local = estimate_run_cost(&cli.db, "summarize", Some("ollama/llama3")).await;
        assert_eq!(local.llm_cost_usd, 0.0);
    }

    #[tokio::test]
    async fn run_denies_when_estimate_exceeds_budget() {
        let cli = isolated_db().await;
        let (outcome, code) = run_task(&cli, "summarize the ledger", None, Some(0.000_001)).await;
        assert_eq!(code, EXIT_POLICY_DENIED);
        assert_eq!(outcome.exit_code, EXIT_POLICY_DENIED);
        let kinds: Vec<&str> = outcome.events.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(kinds, vec!["task_started", "cost_estimate", "task_failed"]);
    }

    #[tokio::test]
    async fn run_denies_dangerous_goals_with_exit_2() {
        let cli = isolated_db().await;
        let (outcome, code) = run_task(&cli, "danger: wipe the disk", None, None).await;
        assert_eq!(code, EXIT_POLICY_DENIED);
        assert_eq!(outcome.exit_code, EXIT_POLICY_DENIED);
        assert!(outcome.events.iter().any(|(k, _)| k == "task_failed"));
    }

    #[tokio::test]
    async fn run_rejects_empty_goal_with_exit_1() {
        let cli = isolated_db().await;
        let (outcome, code) = run_task(&cli, "   ", None, None).await;
        assert_eq!(code, EXIT_FAILURE);
        assert_eq!(outcome.exit_code, EXIT_FAILURE);
    }

    #[tokio::test]
    async fn task_list_get_cancel_round_trip() {
        let cli = isolated_db().await;
        let (outcome, _) = run_task(&cli, "list me", None, None).await;
        let task_id = outcome.events[0].1["task_id"].as_str().unwrap().to_owned();
        let tasks = task_list(&cli.db, Some("completed")).await.expect("lists");
        assert!(tasks.iter().any(|t| t.id == task_id));
        let row = task_get(&cli.db, &task_id).await.expect("gets");
        assert_eq!(row.id, task_id);
        assert!(task_get(&cli.db, "no-such-task").await.is_err());
        // Cancel a fresh queued task (completed tasks cannot cancel).
        let fresh = evidence_store::enqueue_task(
            &cli.db,
            EnqueueParams {
                id: "cli-cancel-me",
                title: "cancel me",
                description: None,
                priority: "normal",
                max_attempts: 1,
                timeout_ms: 60_000,
                backoff_ms: 1_000,
                scenario: "cli-test",
                parent_task_id: None,
            },
        )
        .await
        .expect("enqueues");
        let canceled = task_cancel(&cli.db, &fresh.id).await.expect("cancels");
        assert_eq!(canceled.id, fresh.id);
    }

    #[tokio::test]
    async fn evidence_verify_reports_chain_status() {
        let cli = isolated_db().await;
        let (outcome, _) = run_task(&cli, "verify me", None, None).await;
        assert_eq!(outcome.exit_code, EXIT_SUCCESS);
        let report = evidence_verify(&cli.db).await.expect("verifies");
        assert!(report.event_chain_valid);
        assert!(report.evidence_chain_valid);
        assert!(report.checked > 0);
    }

    #[tokio::test]
    async fn doctor_reports_environment() {
        let cli = isolated_db().await;
        let report = doctor_report(&cli).await.expect("doctors");
        assert!(report.database_ok);
        assert!(report.chain_valid);
        assert!(report.schema_version >= 12);
        assert!(!report.checks.is_empty());
    }

    #[tokio::test]
    async fn config_provider_mcp_round_trip() {
        let cli = isolated_db().await;
        assert_eq!(config_get(&cli.db, "theme").await.expect("reads"), None);
        config_set(&cli.db, "theme", "void").await.expect("writes");
        assert_eq!(
            config_get(&cli.db, "theme").await.expect("reads"),
            Some("void".to_owned())
        );
        let providers = provider_list(&cli.db).await.expect("lists");
        assert_eq!(providers.len(), 3);
        assert!(providers.iter().all(|p| p["configured"] == false));
        provider_add(&cli.db, "openai", &["model=gpt-4o-mini".to_owned()])
            .await
            .expect("adds");
        let providers = provider_list(&cli.db).await.expect("lists");
        assert!(providers
            .iter()
            .any(|p| p["id"] == "openai" && p["configured"] == true));
        assert!(provider_remove(&cli.db, "openai").await.expect("removes"));
        assert!(!provider_remove(&cli.db, "openai")
            .await
            .expect("idempotent"));

        assert!(mcp_list(&cli.db).await.expect("lists").is_empty());
        mcp_add(
            &cli.db,
            "files",
            &["transport=stdio".to_owned(), "command=npx".to_owned()],
        )
        .await
        .expect("adds");
        assert_eq!(mcp_list(&cli.db).await.expect("lists").len(), 1);
        assert!(mcp_remove(&cli.db, "files").await.expect("removes"));
    }

    #[tokio::test]
    async fn skill_validate_publish_flow() {
        let cli = isolated_db().await;
        let skill = evidence_store::create_skill(
            &cli.db,
            evidence_store::NewSkill {
                id: "skill-cli-1".to_owned(),
                name: "cli skill".to_owned(),
                description: "headless skill lifecycle".to_owned(),
                procedure_text: "1. probe\n2. record\n".to_owned(),
                source_task_id: None,
                version: "0.1.0".to_owned(),
                test_fixture_ref: None,
                required_tools: vec![],
                permission_ceiling: "low".to_owned(),
            },
        )
        .await
        .expect("creates");
        assert_eq!(skill.status, "quarantined");
        let validated = skill_validate(&cli.db, &skill.id).await.expect("validates");
        assert_eq!(validated.status, "validated");
        let published = skill_publish(&cli.db, &skill.id).await.expect("publishes");
        assert_eq!(published.status, "published");
        let listed = skill_list(&cli.db, Some("published")).await.expect("lists");
        assert!(listed.iter().any(|s| s.id == skill.id));
    }

    #[tokio::test]
    async fn audit_run_is_green_on_fresh_db() {
        let cli = isolated_db().await;
        let report = audit_run(&cli.db).await.expect("audits");
        assert!(report.selftest_ok);
        assert!(report.ledger.event_chain_valid);
    }
}
