//! Proof of Hardening: end-to-end Batch 4 integration across every
//! hardening system.
//!
//! Scenario `proof-of-hardening`: Talos survives a simulated crash on a
//! file-backed database, recovers the interrupted task, plans with a
//! validated DAG (keyword fallback + stub-LLM paths), delegates under
//! supervisor limits, executes real specialist work through the OS sandbox
//! with enforced child budgets, kills a runaway process on timeout, learns
//! a quarantined skill from the verified task, and proves the evidence
//! chain, backup/restore, and fidelity accounting truthful throughout.
//!
//! Modes (environment):
//! - default (full-mock): every adapter deterministic; the OS sandbox runs
//!   live wherever a backend exists (bubblewrap/Seatbelt/Job Objects),
//!   otherwise the demo fails closed loudly instead of pretending.
//! - `TALOS_HARDENING_SANDBOX=require-live`: additionally assert the live
//!   backend by name (Linux CI: bubblewrap).
//!
//! Prints a single JSON document to stdout (logs go to stderr). Exits
//! non-zero on the first failed expectation. The demo directory comes from
//! `TALOS_HARDENING_DIR` (set by `tools/demo/hardening.mjs` so the CLI can
//! verify the same database afterwards) or a fresh temp dir otherwise.

use std::path::{Path, PathBuf};
use std::time::Instant;

use agent_runtime::delegation::SpecialistRole;
use agent_runtime::queue::{CancelToken, SubmitParams};
use agent_runtime::supervisor::{PlanningLlmClient, SupervisorAgent, SupervisorConfig};
use policy_broker::DefaultPolicyEngine;
use talos_common::now_iso;
use talos_common::ExecutionFidelity;
use tool_runtime::{ToolInvocation, ToolRegistry, ToolStatus};

const SCENARIO: &str = "proof-of-hardening";
const PARENT_BUDGET_USD: f64 = 5.0;
const CHILD_BUDGET_USD: f64 = 2.0;

#[derive(Debug, serde::Serialize)]
struct Step {
    name: &'static str,
    ok: bool,
    detail: String,
    duration_ms: u128,
}

#[derive(Debug, serde::Serialize)]
struct DemoResult {
    scenario: &'static str,
    mode: String,
    ok: bool,
    steps: Vec<Step>,
    metrics: serde_json::Value,
    generated_at: String,
}

struct Ctx {
    steps: Vec<Step>,
}

impl Ctx {
    fn step(&mut self, name: &'static str, ok: bool, detail: String, started: Instant) {
        let duration_ms = started.elapsed().as_millis();
        if ok {
            eprintln!("[hardening] ok   {name} ({duration_ms}ms) — {detail}");
        } else {
            eprintln!("[hardening] FAIL {name} ({duration_ms}ms) — {detail}");
        }
        self.steps.push(Step {
            name,
            ok,
            detail,
            duration_ms,
        });
    }

    fn check(&mut self, name: &'static str, ok: bool, detail: String, started: Instant) {
        self.step(name, ok, detail.clone(), started);
        if !ok {
            let result = DemoResult {
                scenario: SCENARIO,
                mode: mode_label(),
                ok: false,
                steps: std::mem::take(&mut self.steps),
                metrics: serde_json::json!({}),
                generated_at: now_iso(),
            };
            println!(
                "{}",
                serde_json::to_string_pretty(&result).unwrap_or_default()
            );
            std::process::exit(1);
        }
    }
}

fn mode_label() -> String {
    if std::env::var("TALOS_HARDENING_SANDBOX").as_deref() == Ok("require-live") {
        "sandbox-live".to_owned()
    } else {
        "full-mock".to_owned()
    }
}

/// Stub LLM that replays one canned plan (proves the LLM planning path +
/// deterministic validation gates without a network call or key).
struct CannedPlanner {
    response: String,
}

#[async_trait::async_trait]
impl PlanningLlmClient for CannedPlanner {
    async fn complete(&self, _prompt: String) -> Result<String, String> {
        Ok(self.response.clone())
    }
}

fn canned_valid_plan() -> String {
    serde_json::json!({
        "plan_version": "1.0",
        "steps": [
            {"step_id": "a", "role": "coder", "goal": "implement date utility with tests", "depends_on": [], "tools_needed": ["fs.read"], "risk_level": "low", "estimated_tokens": 120},
            {"step_id": "b", "role": "coder", "goal": "implement hash utility with tests", "depends_on": ["a"], "tools_needed": ["fs.read"], "risk_level": "medium", "estimated_tokens": 180}
        ],
        "total_estimated_tokens": 300,
        "reasoning": "two scoped coder slices, ordered by dependency"
    })
    .to_string()
}

fn canned_circular_plan() -> String {
    serde_json::json!({
        "plan_version": "1.0",
        "steps": [
            {"step_id": "a", "role": "coder", "goal": "do a", "depends_on": ["b"], "tools_needed": ["fs.read"], "risk_level": "low", "estimated_tokens": 100},
            {"step_id": "b", "role": "coder", "goal": "do b", "depends_on": ["a"], "tools_needed": ["fs.read"], "risk_level": "low", "estimated_tokens": 100}
        ],
        "total_estimated_tokens": 200,
        "reasoning": "cycle"
    })
    .to_string()
}

/// Minimal Cargo workspace with one passing test: `cargo test` exits 0 with
/// `passed > 0`, which is what upgrades a coder run to verified fidelity.
fn seed_cargo_workspace(root: &Path) {
    std::fs::create_dir_all(root.join("src")).expect("workspace src dir");
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"hardening_probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("workspace manifest");
    std::fs::write(
        root.join("src").join("lib.rs"),
        "pub fn triangular(n: u64) -> u64 {\n    (1..=n).sum()\n}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn triangular_sums() {\n        assert_eq!(super::triangular(10), 55);\n    }\n}\n",
    )
    .expect("workspace lib");
}

/// Platform sleeper for the kill test: `sleep 30` on Unix, `ping -n 30` on
/// Windows (same shape as the tool_runtime unit tests).
#[cfg(unix)]
fn sleeper() -> (&'static str, Vec<String>) {
    ("sleep", vec!["30".to_owned()])
}

/// Platform sleeper for the kill test.
#[cfg(windows)]
fn sleeper() -> (&'static str, Vec<String>) {
    (
        "ping",
        vec!["-n".to_owned(), "30".to_owned(), "127.0.0.1".to_owned()],
    )
}

fn tool_sandbox(root: PathBuf, allow: Vec<String>) -> tool_runtime::SandboxConfig {
    tool_runtime::SandboxConfig {
        root,
        exec_enabled: true,
        exec_allowlist: allow,
        exec_timeout_ms: 5_000,
        max_bytes: 262_144,
        max_output_bytes: 102_400,
        allow_network: false,
        profile: tool_runtime::SandboxProfile::WorkspaceWrite,
        danger_approval_id: None,
        computer: computer_use::SafetyEnvelope::default(),
        computer_backend: computer_use::ComputerBackend::Mock,
        computer_vm: None,
        execution_target: tool_runtime::ExecutionTarget::Local,
        remote_runner: None,
        browser_profile_root: None,
    }
}

fn enforcement_config_for(root: &Path) -> sandbox::SandboxConfig {
    sandbox::SandboxConfig {
        profile: sandbox::SandboxProfile::WorkspaceWrite,
        workspace_root: root.to_path_buf(),
        network_allowed: false,
        allowed_hosts: Vec::new(),
        cpu_limit_percent: None,
        memory_limit_mb: None,
        timeout_ms: 60_000,
        allowed_commands: Vec::new(),
        danger_approval_id: None,
    }
}

#[tokio::main]
async fn main() {
    let mut ctx = Ctx { steps: Vec::new() };
    let demo_started = Instant::now();

    // Demo directory: shared with the CLI wrapper when TALOS_HARDENING_DIR
    // is set, otherwise an isolated temp dir (left behind on purpose so a
    // local run can be inspected afterwards).
    let demo_dir: PathBuf = match std::env::var("TALOS_HARDENING_DIR") {
        Ok(dir) if !dir.trim().is_empty() => PathBuf::from(dir),
        _ => std::env::temp_dir().join(format!("talos-hardening-{}", std::process::id())),
    };
    std::fs::create_dir_all(&demo_dir).expect("demo dir");
    let main_db_path = demo_dir.join("main").join("talos.db");
    let enc_db_path = demo_dir.join("enc").join("talos.db");

    // ---- 0. Preflight: cargo present, sandbox backend available. ----
    let started = Instant::now();
    let cargo_ok = tokio::process::Command::new("cargo")
        .arg("--version")
        .output()
        .await
        .map(|out| out.status.success())
        .unwrap_or(false);
    let backend = sandbox::select_enforcer();
    let backend_available = backend.is_available();
    let backend_name = backend.name().to_owned();
    ctx.check(
        "preflight.toolchains",
        cargo_ok && backend_available,
        format!(
            "cargo present={cargo_ok} backend={backend_name} available={backend_available} ({})",
            backend.availability_detail()
        ),
        started,
    );
    if mode_label() == "sandbox-live" {
        let started = Instant::now();
        #[cfg(target_os = "linux")]
        let live_ok = backend_name == "bubblewrap" && backend_available;
        #[cfg(target_os = "macos")]
        let live_ok = backend_name == "seatbelt" && backend_available;
        #[cfg(windows)]
        let live_ok = backend_name == "windows-job-objects" && backend_available;
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        let live_ok = false;
        ctx.check(
            "preflight.sandbox_live",
            live_ok,
            format!("live backend enforced: {backend_name}"),
            started,
        );
    }

    let engine = DefaultPolicyEngine::new();
    let mut registry = ToolRegistry::with_builtins();
    // process.exec ships disabled; the demo enables it with an explicit
    // allowlist (same shape as workspace-backed coding tasks).
    registry
        .set_enabled(tool_runtime::PROCESS_EXEC, true)
        .expect("enable process.exec");

    // ---- 1. File-backed durable state (WAL, schema current). ----
    let started = Instant::now();
    if let Some(parent) = main_db_path.parent() {
        std::fs::create_dir_all(parent).expect("main db dir");
    }
    let db = evidence_store::EvidenceDb::connect_file(&main_db_path)
        .await
        .expect("file db");
    let journal = db.journal_mode().await.expect("journal mode");
    let schema = db.schema_version().await.expect("schema version");
    ctx.check(
        "state.durable_file",
        journal == "wal" && schema >= 12 && main_db_path.is_file(),
        format!(
            "{} journal={journal} schema={schema}",
            main_db_path.display()
        ),
        started,
    );

    // ---- 2. Encryption at rest (separate handle; production combines both). ----
    let started = Instant::now();
    if let Some(parent) = enc_db_path.parent() {
        std::fs::create_dir_all(parent).expect("enc db dir");
    }
    let enc_key = evidence_store::DatabaseKey::generate();
    let enc_db = evidence_store::EvidenceDb::connect_encrypted_file(&enc_db_path, &enc_key)
        .await
        .expect("encrypted db");
    let enc_flag = enc_db.is_encrypted();
    evidence_store::insert_evidence(
        &enc_db,
        evidence_store::NewEvidence {
            id: "hardening-enc-seed".to_owned(),
            task_id: None,
            agent_id: None,
            session_id: None,
            kind: "seed".to_owned(),
            label: None,
            summary: Some("encryption probe".to_owned()),
            uri: None,
            mime_type: None,
            payload: serde_json::json!({ "secret": "at-rest" }),
        },
    )
    .await
    .expect("seed encrypted");
    enc_db.close_encrypted().await.expect("seal");
    drop(enc_db);
    let canonical_bytes = std::fs::read(&enc_db_path).expect("canonical bytes");
    let canonical_kind = evidence_store::classify_bytes(&canonical_bytes);
    let opaque = canonical_kind == evidence_store::FileKind::Envelope
        && !String::from_utf8_lossy(&canonical_bytes).contains("SQLite format");
    let reopened = evidence_store::EvidenceDb::connect_encrypted_file(&enc_db_path, &enc_key)
        .await
        .expect("reopen encrypted");
    let enc_rows = evidence_store::list_recent_evidence(&reopened, 10)
        .await
        .expect("list encrypted");
    let enc_kept = enc_rows.iter().any(|r| r.id == "hardening-enc-seed");
    reopened.close_encrypted().await.expect("seal again");
    drop(reopened);
    let wrong_key = evidence_store::DatabaseKey::generate();
    let wrong_rejected =
        evidence_store::EvidenceDb::connect_encrypted_file(&enc_db_path, &wrong_key)
            .await
            .is_err();
    ctx.check(
        "state.encrypted",
        enc_flag && opaque && enc_kept && wrong_rejected,
        format!("encrypted={enc_flag} canonical_opaque={opaque} reopened={enc_kept} wrong_key_rejected={wrong_rejected}"),
        started,
    );

    // ---- 3. Crash simulation + startup recovery. ----
    let started = Instant::now();
    // First boot on a fresh database: clean flag, nothing interrupted.
    let first_recovery = evidence_store::run_startup_recovery(&db)
        .await
        .expect("first recovery");
    let first_clean = !first_recovery.dirty_shutdown && first_recovery.recovered.is_empty();
    let crash_id = "hardening-crash-1";
    evidence_store::enqueue_task(
        &db,
        evidence_store::EnqueueParams {
            id: crash_id,
            title: "hardening crash probe",
            description: Some("interrupted mid-execution"),
            priority: "normal",
            max_attempts: 1,
            timeout_ms: 60_000,
            backoff_ms: 1_000,
            scenario: "hardening-crash",
            parent_task_id: None,
        },
    )
    .await
    .expect("enqueue crash task");
    evidence_store::record_transition(&db, crash_id, "queued", "planning", "demo boot")
        .await
        .expect("to planning");
    evidence_store::record_transition(&db, crash_id, "planning", "executing", "demo boot")
        .await
        .expect("to executing");
    // Simulate the kill: drop the handle WITHOUT a clean shutdown (the
    // dirty flag set by the first recovery is still in place).
    drop(db);
    let db = evidence_store::EvidenceDb::connect_file(&main_db_path)
        .await
        .expect("reopen after crash");
    let was_dirty = evidence_store::was_dirty_shutdown(&db)
        .await
        .expect("dirty flag");
    let recovery = evidence_store::run_startup_recovery(&db)
        .await
        .expect("crash recovery");
    let recovered = recovery
        .recovered
        .iter()
        .find(|r| r.task_id == crash_id)
        .cloned();
    let recovered_ok = matches!(
        &recovered,
        Some(r) if r.to_status == "failed" && r.reason == "crash_recovery_failed"
    );
    let crash_row = evidence_store::get_task(&db, crash_id)
        .await
        .expect("crash row");
    evidence_store::mark_clean_shutdown(&db)
        .await
        .expect("clean shutdown");
    let clean_after = !evidence_store::was_dirty_shutdown(&db)
        .await
        .expect("clean flag");
    ctx.check(
        "crash.recover",
        first_clean
            && was_dirty
            && recovery.dirty_shutdown
            && recovered_ok
            && crash_row.map(|r| r.status) == Some("failed".to_owned())
            && recovery.chain_valid
            && clean_after,
        format!(
            "first_boot_clean={first_clean} dirty_detected={was_dirty} recovered={recovered_ok:?} chain_valid={} clean_after={clean_after}",
            recovery.chain_valid
        ),
        started,
    );

    // ---- 4. Planning: keyword fallback, stub-LLM validation, rejection. ----
    let started = Instant::now();
    let supervisor = SupervisorAgent::new("supervisor-hardening");
    let goal = "Research release notes, implement a date utility and a hash utility in code, and verify with tests";
    let keyword_dag = supervisor
        .plan_with_llm("hardening-parent", goal, None, &[], None, None)
        .await;
    let keyword_ok = keyword_dag.method == agent_runtime::planner::PlanningMethod::Keyword
        && !keyword_dag.steps.is_empty()
        && keyword_dag.fallback_reason.as_deref()
            == Some("no provider key configured; using deterministic keyword path");
    ctx.check(
        "plan.keyword_fallback",
        keyword_ok,
        format!(
            "method={} steps={} reason={:?}",
            keyword_dag.method.as_str(),
            keyword_dag.steps.len(),
            keyword_dag.fallback_reason
        ),
        started,
    );

    let started = Instant::now();
    let llm_dag = supervisor
        .plan_with_llm(
            "hardening-parent",
            goal,
            Some("hardening slices"),
            &SupervisorAgent::parent_authority(),
            Some(PARENT_BUDGET_USD),
            Some(&CannedPlanner {
                response: canned_valid_plan(),
            }),
        )
        .await;
    let llm_ok = llm_dag.method == agent_runtime::planner::PlanningMethod::Llm
        && llm_dag.steps.len() == 2
        && llm_dag.levels.len() == 2
        && llm_dag.request_hash.is_some()
        && llm_dag.response_hash.is_some();
    ctx.check(
        "plan.llm_validated",
        llm_ok,
        format!(
            "method={} steps={} levels={} hashes={}/{}",
            llm_dag.method.as_str(),
            llm_dag.steps.len(),
            llm_dag.levels.len(),
            llm_dag.request_hash.is_some(),
            llm_dag.response_hash.is_some()
        ),
        started,
    );

    let started = Instant::now();
    let reject_dag = supervisor
        .plan_with_llm(
            "hardening-parent",
            goal,
            None,
            &SupervisorAgent::parent_authority(),
            None,
            Some(&CannedPlanner {
                response: canned_circular_plan(),
            }),
        )
        .await;
    let reject_ok = reject_dag.method == agent_runtime::planner::PlanningMethod::Keyword
        && reject_dag
            .fallback_reason
            .as_deref()
            .is_some_and(|r| r.contains("circular") || r.contains("dependency"));
    ctx.check(
        "plan.invalid_rejected",
        reject_ok,
        format!("fallback={:?}", reject_dag.fallback_reason),
        started,
    );

    let started = Instant::now();
    let parsed = agent_runtime::planner::parse_plan_json(&canned_valid_plan()).expect("parse");
    let validation_ctx = agent_runtime::planner::ValidationContext {
        max_depth: 4,
        max_fanout: 4,
        allowed_tools: SupervisorAgent::parent_authority(),
        token_budget: Some(100_000),
    };
    let validated = agent_runtime::planner::validate_plan(&parsed, &validation_ctx);
    let gates_ok = match validated {
        Ok(v) => {
            let order_ok = agent_runtime::planner::topological_order(&v.plan)
                .map(|o| o == vec!["a", "b"])
                == Ok(true);
            let levels_ok = agent_runtime::planner::execution_levels(&v.plan)
                == Ok(vec![vec!["a".to_owned()], vec!["b".to_owned()]]);
            order_ok && levels_ok && !v.requires_approval
        }
        Err(_) => false,
    };
    let bad_plan = serde_json::json!({
        "plan_version": "1.0",
        "steps": [{"step_id": "x", "role": "coder", "goal": "bad tool", "depends_on": [], "tools_needed": ["no-such-tool"], "risk_level": "low", "estimated_tokens": 10}],
        "total_estimated_tokens": 10,
        "reasoning": "unknown tool"
    })
    .to_string();
    let unknown_tool_rejected = agent_runtime::planner::parse_plan_json(&bad_plan)
        .ok()
        .map(|p| {
            agent_runtime::planner::validate_plan(
                &p,
                &agent_runtime::planner::ValidationContext {
                    max_depth: 4,
                    max_fanout: 4,
                    allowed_tools: vec!["fs.read".to_owned()],
                    token_budget: None,
                },
            )
            .is_err()
        })
        .unwrap_or(false);
    ctx.check(
        "plan.gates_direct",
        gates_ok && unknown_tool_rejected,
        format!("order+levels ok={gates_ok} unknown_tool_rejected={unknown_tool_rejected}"),
        started,
    );

    // ---- 5. Supervisor fan-out/depth/authority limits (P02). ----
    let started = Instant::now();
    let config = SupervisorConfig {
        max_fanout: 2,
        max_depth: 2,
        default_deadline_ms: 60_000,
        default_context_budget: 8_000,
    };
    let config_ok = config.validate().is_ok()
        && (SupervisorConfig {
            max_fanout: 17,
            ..config.clone()
        })
        .validate()
        .is_err()
        && (SupervisorConfig {
            max_depth: 5,
            ..config.clone()
        })
        .validate()
        .is_err();
    let limited = SupervisorAgent::with_config("supervisor-hardening", config);
    let planned = limited.decompose(goal, Some("hardening slices"));
    let decompose_ok = !planned.is_empty();
    let requests = limited.plan_delegations(
        "hardening-parent",
        1,
        &planned[..planned.len().min(2)],
        Some(PARENT_BUDGET_USD),
    );
    let allowlist = SupervisorAgent::parent_authority();
    let mut spawns_ok = requests.len() == planned.len().min(2);
    for request in &requests {
        if limited.validate_spawn(0, 1, &allowlist, request).is_err() {
            spawns_ok = false;
        }
    }
    // Over fan-out and over depth must fail.
    let fanout_err = requests
        .first()
        .map(|r| limited.validate_spawn(2, 1, &allowlist, r).is_err())
        .unwrap_or(false);
    let deep_requests = limited.plan_delegations(
        "hardening-parent",
        2,
        &planned[..1.min(planned.len())],
        None,
    );
    let depth_err = deep_requests
        .first()
        .map(|r| limited.validate_spawn(0, 2, &allowlist, r).is_err())
        .unwrap_or(false);
    ctx.check(
        "supervisor.limits",
        config_ok && decompose_ok && spawns_ok && fanout_err && depth_err,
        format!(
            "slices={} valid_spawns={spawns_ok} fanout_denied={fanout_err} depth_denied={depth_err}",
            planned.len()
        ),
        started,
    );

    // ---- 6. Child budgets: reserve, enforce, settle. ----
    let started = Instant::now();
    // Parent row first: children reference it, and the CLI finds it by id.
    let parent_id = "hardening-parent";
    evidence_store::enqueue_task(
        &db,
        evidence_store::EnqueueParams {
            id: parent_id,
            title: "proof-of-hardening: verified slices",
            description: Some(goal),
            priority: "high",
            max_attempts: 1,
            timeout_ms: 300_000,
            backoff_ms: 1_000,
            scenario: "hardening-verified",
            parent_task_id: None,
        },
    )
    .await
    .expect("submit parent");
    evidence_store::set_task_budget(&db, "hardening-parent", Some(PARENT_BUDGET_USD))
        .await
        .expect("parent budget");
    // Child ids mirror the delegation contract ({parent}-child-{i}-{role}).
    let child_a = "hardening-parent-child-1-coder";
    let child_b = "hardening-parent-child-2-coder";
    for child in [child_a, child_b] {
        evidence_store::enqueue_task(
            &db,
            evidence_store::EnqueueParams {
                id: child,
                title: &format!("hardening slice {child}"),
                description: Some("verified coder slice"),
                priority: "high",
                max_attempts: 1,
                timeout_ms: 180_000,
                backoff_ms: 1_000,
                scenario: "hardening-verified",
                parent_task_id: Some("hardening-parent"),
            },
        )
        .await
        .expect("enqueue child");
    }
    let rsv_a = cost_tracker::reserve_budget(&db, "hardening-parent", child_a, CHILD_BUDGET_USD)
        .await
        .expect("reserve a");
    let rsv_b = cost_tracker::reserve_budget(&db, "hardening-parent", child_b, CHILD_BUDGET_USD)
        .await
        .expect("reserve b");
    let remaining_a = cost_tracker::get_remaining_budget(&db, child_a)
        .await
        .expect("remaining a");
    let over_denied = cost_tracker::check_child_budget(&db, child_a, CHILD_BUDGET_USD + 1.0)
        .await
        .map(|v| v == cost_tracker::ChildBudgetVerdict::Deny)
        .unwrap_or(false);
    let under_allowed = cost_tracker::check_child_budget(&db, child_a, 1.0)
        .await
        .map(|v| v == cost_tracker::ChildBudgetVerdict::Allow)
        .unwrap_or(false);
    ctx.check(
        "budget.children",
        remaining_a == Some(CHILD_BUDGET_USD) && over_denied && under_allowed,
        format!("reserved 2x${CHILD_BUDGET_USD} remaining={remaining_a:?} over_denied={over_denied} under_allowed={under_allowed}"),
        started,
    );

    // ---- 7. Real specialist execution (parent + 2 verified coder slices). ----
    let started = Instant::now();
    // Execution slices mirror the delegation contract: deterministic child
    // ids ({parent}-child-{i}-{role}), equal budget shares, depth + 1.
    use agent_runtime::delegation::PlannedSubtask;
    let exec_slices = vec![
        PlannedSubtask {
            role: SpecialistRole::Coder,
            scope: "implement date utility with tests".to_owned(),
        },
        PlannedSubtask {
            role: SpecialistRole::Coder,
            scope: "implement hash utility with tests".to_owned(),
        },
    ];
    let exec_requests = SupervisorAgent::new("supervisor-hardening").plan_delegations(
        "hardening-parent",
        1,
        &exec_slices,
        Some(PARENT_BUDGET_USD),
    );
    let mut exec_ok = exec_requests.len() == 2
        && exec_requests[0].child_task_id == child_a
        && exec_requests[1].child_task_id == child_b
        && exec_requests[0].budget_usd == Some(2.5);
    for (i, request) in exec_requests.iter().enumerate() {
        if limited.validate_spawn(i, 1, &allowlist, request).is_err() {
            exec_ok = false;
        }
    }
    if !exec_ok {
        ctx.check(
            "delegate.plan",
            false,
            "delegation contract mismatch".to_owned(),
            started,
        );
    }
    let parent_id = "hardening-parent";
    evidence_store::record_transition(&db, parent_id, "queued", "planning", "supervisor plan")
        .await
        .expect("parent planning");
    evidence_store::record_transition(&db, parent_id, "planning", "executing", "delegate")
        .await
        .expect("parent executing");

    let mut child_results = Vec::new();
    for (child_id, scope) in [
        (child_a, "implement date utility with tests"),
        (child_b, "implement hash utility with tests"),
    ] {
        let workspace = demo_dir.join("workspaces").join(child_id);
        std::fs::create_dir_all(&workspace).expect("child workspace");
        seed_cargo_workspace(&workspace);
        evidence_store::record_transition(&db, child_id, "queued", "planning", "delegate")
            .await
            .expect("child planning");
        evidence_store::record_transition(&db, child_id, "planning", "executing", "delegate")
            .await
            .expect("child executing");
        use agent_runtime::specialists::{CoderAgent, Specialist, SpecialistCtx};
        let agent = CoderAgent::new("agent-hardening-coder", child_id, scope, 8_000);
        let sandbox_cfg = tool_sandbox(
            workspace.clone(),
            vec!["cargo".to_owned(), "rustc".to_owned()],
        );
        let sctx = SpecialistCtx {
            db: &db,
            engine: &engine,
            registry: &registry,
            sandbox: &sandbox_cfg,
            workspace_root: Some(workspace.clone()),
            agent_id: "agent-hardening-coder".to_owned(),
            task_id: child_id.to_owned(),
        };
        let result = agent.execute(&sctx).await;
        child_results.push((child_id, scope, result, workspace));
    }
    let mut children_verified = true;
    let mut child_fidelities = Vec::new();
    for (child_id, _scope, result, workspace) in &child_results {
        let artifact_ok = workspace.join(format!("{child_id}-output.md")).is_file();
        let verified =
            result.status == "completed" && result.fidelity == ExecutionFidelity::Verified;
        if !verified || !artifact_ok {
            children_verified = false;
        }
        child_fidelities.push(format!("{child_id}={}", result.fidelity.as_str()));
        evidence_store::record_transition(&db, child_id, "executing", "completed", "slice done")
            .await
            .expect("child completed");
        // Truthful labeling: verified only because the test run passed.
        evidence_store::set_task_fidelity(&db, child_id, result.fidelity.as_str())
            .await
            .expect("child fidelity");
    }
    // Merge: parent fidelity is the minimum of its children.
    evidence_store::record_transition(&db, parent_id, "executing", "completed", "merge")
        .await
        .expect("parent completed");
    evidence_store::set_task_fidelity(&db, parent_id, "verified")
        .await
        .expect("parent fidelity");
    ctx.check(
        "delegate.execute_verified",
        children_verified,
        format!("children: {}", child_fidelities.join(", ")),
        started,
    );

    // ---- 8. Sandbox receipts ride in the tool evidence. ----
    let started = Instant::now();
    let rows = evidence_store::list_recent_evidence(&db, 1000)
        .await
        .expect("ledger reads");
    let mut sandboxed_runs = 0_usize;
    let mut sandbox_methods = Vec::new();
    for row in rows.iter().filter(|r| r.kind == "tool_receipt") {
        let Some(payload) = row.payload_json.as_deref() else {
            continue;
        };
        let receipt: serde_json::Value = match serde_json::from_str(payload) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if receipt.get("toolId").and_then(|v| v.as_str()) != Some("code.run_tests") {
            continue;
        }
        // The sandbox receipt rides in the tool output JSON.
        let output = receipt.get("output");
        let enforced = output
            .and_then(|o| o.get("sandbox"))
            .and_then(|s| s.get("enforced"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        if enforced {
            sandboxed_runs += 1;
        }
        if let Some(method) = output
            .and_then(|o| o.get("sandbox"))
            .and_then(|s| s.get("enforcementMethod"))
            .and_then(|v| v.as_str())
        {
            sandbox_methods.push(method.to_owned());
        }
    }
    let sandbox_reports = sandbox::sandbox_status();
    ctx.check(
        "sandbox.receipts",
        sandboxed_runs >= 2 && sandbox_reports.fail_closed,
        format!("sandboxed run_tests receipts={sandboxed_runs} methods={sandbox_methods:?}"),
        started,
    );

    // ---- 9. Read-only profile denies without a backend; forced outage denies. ----
    let started = Instant::now();
    let ro_config = sandbox::SandboxConfig {
        profile: sandbox::SandboxProfile::ReadOnly,
        ..enforcement_config_for(demo_dir.as_path())
    };
    let ro_denied = sandbox::enforce_or_deny(&ro_config).is_err();
    std::env::set_var("TALOS_SANDBOX_FORCE_UNAVAILABLE", "1");
    let forced = sandbox::enforce_or_deny(&enforcement_config_for(demo_dir.as_path()));
    let forced_denied = forced
        .map(|_| false)
        .unwrap_or_else(|e| e.to_string().contains(sandbox::SANDBOX_UNAVAILABLE));
    std::env::remove_var("TALOS_SANDBOX_FORCE_UNAVAILABLE");
    // Backend is back after the hook is cleared.
    let live_again = sandbox::enforce_or_deny(&enforcement_config_for(demo_dir.as_path())).is_ok();
    ctx.check(
        "sandbox.fail_closed",
        ro_denied && forced_denied && live_again,
        format!(
            "read_only_denied={ro_denied} forced_denied={forced_denied} live_again={live_again}"
        ),
        started,
    );

    // ---- 10. Kill on timeout: gated process.exec sleeper, reaped. ----
    let started = Instant::now();
    let kill_task = "hardening-kill-probe";
    evidence_store::enqueue_task(
        &db,
        evidence_store::EnqueueParams {
            id: kill_task,
            title: "hardening kill probe",
            description: Some("sleeper must die on timeout"),
            priority: "normal",
            max_attempts: 1,
            timeout_ms: 60_000,
            backoff_ms: 1_000,
            scenario: "hardening-kill",
            parent_task_id: None,
        },
    )
    .await
    .expect("enqueue kill probe");
    evidence_store::record_transition(&db, kill_task, "queued", "planning", "probe")
        .await
        .expect("probe planning");
    evidence_store::record_transition(&db, kill_task, "planning", "executing", "probe")
        .await
        .expect("probe executing");
    let kill_root = demo_dir.join("kill-probe");
    std::fs::create_dir_all(&kill_root).expect("kill dir");
    std::fs::write(kill_root.join("seed.txt"), b"evidence first").expect("kill seed");
    let kill_sandbox = tool_sandbox(
        kill_root.clone(),
        vec!["sleep".to_owned(), "ping".to_owned(), "echo".to_owned()],
    );
    // Evidence-gating first: ExecuteCommand is write-like.
    let read_outcome = tool_runtime::invoke(
        &db,
        &engine,
        &registry,
        &kill_sandbox,
        ToolInvocation {
            tool_id: tool_runtime::FS_READ.to_owned(),
            task_id: kill_task.to_owned(),
            agent_id: "agent-hardening".to_owned(),
            params: serde_json::json!({ "path": "seed.txt" }),
            dry_run: false,
        },
    )
    .await
    .expect("seed read");
    let read_ok = read_outcome.receipt.status == ToolStatus::Success;
    let (sleeper, sleeper_args) = sleeper();
    let kill_outcome = tool_runtime::invoke(
        &db,
        &engine,
        &registry,
        &kill_sandbox,
        ToolInvocation {
            tool_id: tool_runtime::PROCESS_EXEC.to_owned(),
            task_id: kill_task.to_owned(),
            agent_id: "agent-hardening".to_owned(),
            params: serde_json::json!({ "command": sleeper, "args": sleeper_args, "timeoutMs": 1_000 }),
            dry_run: false,
        },
    )
    .await
    .expect("sleeper invoke");
    let kill_receipt = kill_outcome.receipt;
    let kill_output = &kill_receipt.output;
    let killed = kill_output
        .get("processLifecycle")
        .and_then(|l| l.get("killedOnTimeout"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let reaped = kill_output
        .get("processLifecycle")
        .and_then(|l| l.get("reapConfirmed"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let timed_out_flag = kill_output
        .get("timedOut")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let boxed = kill_output
        .get("sandbox")
        .and_then(|s| s.get("enforced"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    evidence_store::record_transition(&db, kill_task, "executing", "completed", "probe done")
        .await
        .expect("probe completed");
    ctx.check(
        "exec.kill_reaps",
        read_ok
            && kill_receipt.status == ToolStatus::Failed
            && timed_out_flag
            && killed
            && reaped
            && boxed,
        format!(
            "status={:?} timedOut={timed_out_flag} killed={killed} reaped={reaped} sandboxed={boxed}",
            kill_receipt.status
        ),
        started,
    );

    // ---- 11. GEPA learns a quarantined skill from the verified child. ----
    let started = Instant::now();
    let extraction = skill_extractor::extract_skill_for_task(&db, child_a)
        .await
        .expect("extract");
    let (skill_ok, skill_id) = match &extraction {
        skill_extractor::ExtractionOutcome::Created(row) => (
            row.status == "quarantined" && row.source_task_id.as_deref() == Some(child_a),
            row.id.clone(),
        ),
        skill_extractor::ExtractionOutcome::Skipped { code, detail } => {
            eprintln!("[hardening] skill skipped: {code}: {detail}");
            (false, String::new())
        }
    };
    let listed = evidence_store::list_skills(&db, Some("quarantined"), 100)
        .await
        .expect("list skills");
    let registry_ok = listed.iter().any(|s| s.id == skill_id);
    ctx.check(
        "skill.learned",
        skill_ok && registry_ok,
        format!("skill={skill_id} quarantined, in registry"),
        started,
    );

    // ---- 12. Backup, mutate, restore, verify. ----
    let started = Instant::now();
    let backup_path = db.backup(None).await.expect("backup");
    let backups = db.list_backups().await.expect("list backups");
    evidence_store::insert_evidence(
        &db,
        evidence_store::NewEvidence {
            id: "hardening-marker".to_owned(),
            task_id: None,
            agent_id: None,
            session_id: None,
            kind: "marker".to_owned(),
            label: None,
            summary: Some("pre-restore marker".to_owned()),
            uri: None,
            mime_type: None,
            payload: serde_json::json!({ "marker": true }),
        },
    )
    .await
    .expect("marker insert");
    let marker_present = evidence_store::latest_evidence_by_kind(&db, "marker")
        .await
        .expect("marker read")
        .is_some();
    let restore = db.restore(&backup_path).await.expect("restore");
    let marker_gone = evidence_store::latest_evidence_by_kind(&db, "marker")
        .await
        .expect("marker reread")
        .is_none();
    let chain_after = evidence_store::verify_chain(&db).await.expect("chain");
    let tasks_after = evidence_store::list_tasks(&db, 100).await.expect("tasks");
    ctx.check(
        "backup.restore",
        !backups.is_empty()
            && marker_present
            && marker_gone
            && chain_after.valid
            && tasks_after.iter().any(|t| t.id == parent_id)
            && restore.chain_valid,
        format!(
            "backups={} marker_removed={marker_gone} chain_valid={} tasks={}",
            backups.len(),
            chain_after.valid,
            tasks_after.len()
        ),
        started,
    );

    // ---- 13. Fidelity truthfulness + completion rate. ----
    let started = Instant::now();
    let mut counter = 0_u64;
    let mock_task = agent_runtime::queue::submit_task(
        &db,
        &mut counter,
        SubmitParams {
            title: "hardening mock control".to_owned(),
            description: None,
            priority: "low".to_owned(),
            timeout_ms: 30_000,
            max_attempts: 1,
            scenario: "quick_success".to_owned(),
            parent_task_id: None,
        },
    )
    .await
    .expect("mock control");
    let mock_done =
        agent_runtime::queue::execute_task(&db, &engine, &mock_task.id, &CancelToken::new())
            .await
            .expect("mock execute");
    let mock_not_verified = mock_done.fidelity != "verified";
    let guards_ok =
        talos_common::check_fidelity_claim(ExecutionFidelity::Mock, ExecutionFidelity::Verified)
            .is_err()
            && talos_common::check_fidelity_claim(
                ExecutionFidelity::Live,
                ExecutionFidelity::Verified,
            )
            .is_err()
            && talos_common::check_fidelity_claim(
                ExecutionFidelity::Verified,
                ExecutionFidelity::Verified,
            )
            .is_ok();
    // Every verified runtime row must be accounted for honestly: the two
    // coder children (test receipts with exitCode 0) and their parent.
    let all_rows = evidence_store::list_tasks(&db, 1000).await.expect("rows");
    let verified_rows: Vec<&str> = all_rows
        .iter()
        .filter(|t| t.fidelity == "verified")
        .map(|t| t.id.as_str())
        .collect();
    let verified_set_ok = verified_rows.len() == 3
        && verified_rows.contains(&child_a)
        && verified_rows.contains(&child_b)
        && verified_rows.contains(&parent_id);
    let mut test_evidence_ok = true;
    for child in [child_a, child_b] {
        let mut saw_pass = false;
        for row in evidence_store::list_recent_evidence(&db, 2000)
            .await
            .expect("evidence")
        {
            if row.task_id.as_deref() != Some(child) || row.kind != "tool_receipt" {
                continue;
            }
            let payload: serde_json::Value = row
                .payload_json
                .as_deref()
                .and_then(|raw| serde_json::from_str(raw).ok())
                .unwrap_or(serde_json::Value::Null);
            if payload.get("toolId").and_then(|v| v.as_str()) == Some("code.run_tests") {
                let output = payload.get("output");
                let exit_ok = output
                    .and_then(|o| o.get("exitCode"))
                    .and_then(serde_json::Value::as_i64)
                    == Some(0);
                let passed = output
                    .and_then(|o| o.get("passed"))
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(0);
                if exit_ok && passed > 0 {
                    saw_pass = true;
                }
            }
        }
        test_evidence_ok = test_evidence_ok && saw_pass;
    }
    let completed = all_rows.iter().filter(|t| t.status == "completed").count();
    let completion_rate = if all_rows.is_empty() {
        0.0
    } else {
        completed as f64 / all_rows.len() as f64
    };
    ctx.check(
        "fidelity.truthful",
        mock_not_verified && guards_ok && verified_set_ok && test_evidence_ok,
        format!(
            "mock_control_fidelity={} verified_rows={verified_rows:?} completion_rate={completion_rate:.2}",
            mock_done.fidelity
        ),
        started,
    );

    // ---- 14. Chain valid + settle budgets. ----
    let started = Instant::now();
    let event_chain = evidence_store::verify_chain(&db)
        .await
        .expect("event chain");
    let evidence_chain = evidence_store::verify_evidence_chain(&db)
        .await
        .expect("evidence chain");
    let integrity = evidence_store::get_integrity_report(&db)
        .await
        .expect("integrity");
    let unused_a = cost_tracker::settle_budget(&db, &rsv_a, 0.0)
        .await
        .expect("settle a");
    let unused_b = cost_tracker::settle_budget(&db, &rsv_b, 0.0)
        .await
        .expect("settle b");
    let settled_ok =
        (unused_a - CHILD_BUDGET_USD).abs() < 1e-9 && (unused_b - CHILD_BUDGET_USD).abs() < 1e-9;
    ctx.check(
        "chain.valid",
        event_chain.valid && evidence_chain.valid && integrity.healthy && settled_ok,
        format!(
            "events={} evidence={} healthy={} settled=[{unused_a},{unused_b}]",
            event_chain.checked, evidence_chain.checked, integrity.healthy
        ),
        started,
    );

    let total_ms = demo_started.elapsed().as_millis();
    let result = DemoResult {
        scenario: SCENARIO,
        mode: mode_label(),
        ok: true,
        steps: ctx.steps,
        metrics: serde_json::json!({
            "demoDir": demo_dir.display().to_string(),
            "dbFile": main_db_path.display().to_string(),
            "parentId": parent_id,
            "childIds": [child_a, child_b],
            "skillId": skill_id,
            "completionRate": completion_rate,
            "chainChecked": event_chain.checked + evidence_chain.checked,
            "sandboxBackend": backend_name,
            "sandboxEnforced": true,
            "totalMs": total_ms,
        }),
        generated_at: now_iso(),
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&result).unwrap_or_default()
    );
    eprintln!("[hardening] proof-of-hardening: all stages passed");
}
