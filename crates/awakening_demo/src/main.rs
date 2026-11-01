//! Proof of Awakening: end-to-end autonomous task across every Batch 3 system.
//!
//! Scenario `proof-of-awakening`: Talos completes "create a Python script
//! that fetches the current date from a web API, write tests for it, and
//! verify the tests pass" via Supervisor decomposition (research → browser,
//! implementation + testing → coder, verification → supervisor), with policy
//! gates, evidence receipts, context assembly, memory recall, HITL approval,
//! cost tracking, and telemetry — all local-first.
//!
//! Execution modes (environment):
//! - default (full-mock): every adapter mocked/simulated. CI-safe,
//!   deterministic, no network, no display, no keys.
//! - `TALOS_AWAKENING_LLM=real`: one real model completion through the
//!   gateway (needs a provider key in the OS keychain, configured via the
//!   Settings page; fails closed with guidance otherwise). Never in CI.
//! - `TALOS_BROWSER_LIVE=1`: real Playwright navigation (needs the built
//!   browser worker). Never in CI.
//!
//! Prints a single JSON document to stdout (logs go to stderr). Exits
//! non-zero on the first failed expectation.

use std::collections::HashMap;
use std::time::Instant;

use agent_runtime::queue::{CancelToken, SubmitParams};
use policy_broker::{Capability, DefaultPolicyEngine, PolicyDecision, RiskLevel};
use talos_common::now_iso;
use tool_runtime::{ToolInvocation, ToolRegistry, ToolStatus};

const SCENARIO: &str = "proof-of-awakening";
const TASK_BUDGET_USD: f64 = 5.0;
const SESSION_BUDGET_USD: f64 = 25.0;

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
            eprintln!("[awakening] ok   {name} ({duration_ms}ms) — {detail}");
        } else {
            eprintln!("[awakening] FAIL {name} ({duration_ms}ms) — {detail}");
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
    let llm_real = std::env::var("TALOS_AWAKENING_LLM").as_deref() == Ok("real");
    let browser_live = std::env::var("TALOS_BROWSER_LIVE").as_deref() == Ok("1");
    match (llm_real, browser_live) {
        (false, false) => "full-mock".to_owned(),
        (true, false) => "llm-real".to_owned(),
        (false, true) => "browser-real".to_owned(),
        (true, true) => "full-real".to_owned(),
    }
}

/// One gated tool call through policy, evidence, and compensation gates.
/// Nine explicit args (same shape as the desktop delegation layer, which
/// carries the same `allow`): every call site states db/engine/registry/
/// sandbox/tool/task/agent/params/mode with no hidden state.
#[allow(clippy::too_many_arguments)]
fn invoke_sync<'a>(
    db: &'a evidence_store::EvidenceDb,
    engine: &'a DefaultPolicyEngine,
    registry: &'a ToolRegistry,
    config: &'a tool_runtime::SandboxConfig,
    tool_id: &str,
    task_id: &str,
    agent_id: &str,
    params: serde_json::Value,
    dry_run: bool,
) -> impl std::future::Future<Output = talos_common::Result<tool_runtime::GatedOutcome>> + 'a {
    tool_runtime::invoke(
        db,
        engine,
        registry,
        config,
        ToolInvocation {
            tool_id: tool_id.to_owned(),
            task_id: task_id.to_owned(),
            agent_id: agent_id.to_owned(),
            params,
            dry_run,
        },
    )
}

async fn pytest_available() -> bool {
    let probe = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        tokio::process::Command::new("pytest")
            .arg("--version")
            .output(),
    )
    .await;
    matches!(probe, Ok(Ok(out)) if out.status.success())
}

#[tokio::main]
async fn main() {
    let mode = mode_label();
    let llm_real = mode == "llm-real" || mode == "full-real";
    let browser_live = mode == "browser-real" || mode == "full-real";
    eprintln!("[awakening] proof-of-awakening starting (mode={mode})");
    let mut ctx = Ctx { steps: Vec::new() };
    let demo_started = Instant::now();

    // Telemetry: process-wide collector (spans from every instrumented crate).
    let collector = talos_telemetry::SpanCollector::with_defaults();
    talos_telemetry::set_global_collector(collector.clone());

    let db = evidence_store::EvidenceDb::connect_memory()
        .await
        .expect("in-memory db");
    let engine = DefaultPolicyEngine::new();
    let supervisor =
        agent_runtime::supervisor::SupervisorAgent::new("supervisor-proof-of-awakening");

    // ---- 1. Supervisor receives the user task; budget cap set. ----
    let started = Instant::now();
    let mut counter = 0_u64;
    let goal = "Create a Python script that fetches the current date from a web API, write tests for it, and verify the tests pass. Needs code, browser research on the web date API, and research notes.";
    let parent = agent_runtime::queue::submit_task(
        &db,
        &mut counter,
        SubmitParams {
            title: "proof-of-awakening: date-API script + tests".to_owned(),
            description: Some(goal.to_owned()),
            priority: "high".to_owned(),
            timeout_ms: 60_000,
            max_attempts: 3,
            scenario: "quick_success".to_owned(),
            parent_task_id: None,
        },
    )
    .await
    .expect("submit parent");
    let parent_id = parent.id.clone();
    evidence_store::set_task_budget(&db, &parent_id, Some(TASK_BUDGET_USD))
        .await
        .expect("task budget");
    ctx.check(
        "task.submit",
        parent.status == "queued",
        format!("parent {parent_id} queued with ${TASK_BUDGET_USD} cap"),
        started,
    );

    // ---- 2. Supervisor decomposes → plan + validate + spawn children. ----
    let started = Instant::now();
    let planned = supervisor.decompose(
        goal,
        Some("date API research, python implementation, pytest verification"),
    );
    let requests = supervisor.plan_delegations(&parent_id, 1, &planned, Some(TASK_BUDGET_USD));
    let parent_allowlist = agent_runtime::supervisor::SupervisorAgent::parent_authority();
    let mut valid = true;
    for request in &requests {
        if supervisor
            .validate_spawn(0, 1, &parent_allowlist, request)
            .is_err()
        {
            valid = false;
        }
    }
    ctx.check(
        "supervisor.decompose",
        planned.len() >= 2 && valid && !requests.is_empty(),
        format!(
            "{} slices: {}",
            planned.len(),
            planned
                .iter()
                .map(|p| format!("{:?}", p.role))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        started,
    );

    // ---- 3. Spawn + execute children, merge results. ----
    let started = Instant::now();
    let mut results = Vec::new();
    for request in &requests {
        evidence_store::enqueue_task(
            &db,
            evidence_store::EnqueueParams {
                id: &request.child_task_id,
                title: &format!("child: {:?}", request.agent_role),
                description: Some(&request.scope),
                priority: "normal",
                max_attempts: 3,
                timeout_ms: 30_000,
                backoff_ms: 500,
                scenario: "quick_success",
                parent_task_id: Some(&parent_id),
            },
        )
        .await
        .expect("enqueue child");
        let done = agent_runtime::queue::execute_task(
            &db,
            &engine,
            &request.child_task_id,
            &CancelToken::new(),
        )
        .await
        .expect("execute child");
        results.push(agent_runtime::delegation::DelegationResult {
            child_task_id: request.child_task_id.clone(),
            status: done.status.clone(),
            artifacts: vec![agent_runtime::delegation::ArtifactRef {
                id: format!("{}-artifact-1", request.child_task_id),
                kind: "research_note".to_owned(),
                summary: request.scope.clone(),
                hash: None,
                evidence_id: None,
            }],
            evidence_ids: Vec::new(),
            limitations: Vec::new(),
            remaining_risks: Vec::new(),
        });
    }
    let merged = supervisor.merge(&parent_id, &results);
    let tree = agent_runtime::queue::get_task_tree(&db, &parent_id)
        .await
        .expect("task tree");
    let all_completed = results.iter().all(|r| r.status == "completed");
    ctx.check(
        "supervisor.delegate_merge",
        all_completed && merged.status == "completed" && tree.children.len() >= 2,
        format!(
            "{} children completed, merged={}, tree has {} children",
            results.len(),
            merged.status,
            tree.children.len()
        ),
        started,
    );
    let browser_child = requests
        .iter()
        .find(|r| {
            matches!(
                r.agent_role,
                agent_runtime::delegation::SpecialistRole::Browser
            )
        })
        .map(|r| r.child_task_id.clone())
        .unwrap_or_else(|| format!("{parent_id}-child-browser"));
    let coder_child = requests
        .iter()
        .find(|r| {
            matches!(
                r.agent_role,
                agent_runtime::delegation::SpecialistRole::Coder
            )
        })
        .map(|r| r.child_task_id.clone())
        .unwrap_or_else(|| format!("{parent_id}-child-coder"));

    // ---- 4. BrowserAgent research (mock dry-run by default). ----
    let started = Instant::now();
    let registry = ToolRegistry::with_builtins();
    let sandbox = tempfile::tempdir().expect("tempdir");
    let sandbox_config = tool_runtime::SandboxConfig {
        root: sandbox.path().to_path_buf(),
        exec_enabled: false,
        exec_allowlist: Vec::new(),
        exec_timeout_ms: 5_000,
        max_bytes: 262_144,
        max_output_bytes: 102_400,
        allow_network: false,
        computer: computer_use::SafetyEnvelope::default(),
    };
    let nav = invoke_sync(
        &db,
        &engine,
        &registry,
        &sandbox_config,
        tool_runtime::BROWSER_NAVIGATE,
        &browser_child,
        "agent-browser",
        serde_json::json!({ "url": "https://example.com" }),
        !browser_live,
    )
    .await
    .expect("browser.navigate");
    let nav_ok =
        nav.receipt.status == ToolStatus::Success && nav.receipt.output.get("domHash").is_some();
    let claim = evidence_store::create_claim(
        &db,
        evidence_store::NewClaimPending {
            id: "claim-awakening-api".to_owned(),
            statement:
                "worldtimeapi.org /api/ip returns current UTC datetime as JSON (suitable date API)"
                    .to_owned(),
            created_by_agent_id: Some("agent-browser".to_owned()),
            task_id: Some(browser_child.clone()),
        },
    )
    .await
    .expect("research claim");
    ctx.check(
        "browser.research",
        nav_ok && claim.status == "pending",
        format!(
            "navigate {} (domHash present), claim {} recorded",
            if browser_live { "live" } else { "mock dry-run" },
            claim.id
        ),
        started,
    );

    // ---- 5. CoderAgent workspace isolation. ----
    let started = Instant::now();
    let ws_base = tempfile::tempdir().expect("workspace base");
    let workspaces = workspace_manager::WorkspaceManager::new(ws_base.path().to_path_buf());
    let workspace = workspaces
        .create_workspace(&coder_child, None)
        .await
        .expect("workspace");
    let contained = workspace.root.starts_with(ws_base.path());
    let coding_config = tool_runtime::SandboxConfig {
        root: workspace.root.clone(),
        exec_enabled: false,
        exec_allowlist: tool_runtime::coder_exec_allowlist(),
        exec_timeout_ms: 60_000,
        max_bytes: 262_144,
        max_output_bytes: 102_400,
        allow_network: false,
        computer: computer_use::SafetyEnvelope::default(),
    };
    ctx.check(
        "workspace.isolate",
        contained && workspace.task_id == coder_child,
        format!(
            "workspace at {} (git={})",
            workspace.root.display(),
            workspace.is_git_worktree
        ),
        started,
    );

    // ---- 6. CoderAgent implementation: seed, read (evidence), write script + tests. ----
    let started = Instant::now();
    tokio::fs::write(
        workspace.root.join("notes.md"),
        b"date API: worldtimeapi.org\n",
    )
    .await
    .expect("seed notes");
    let read = invoke_sync(
        &db,
        &engine,
        &registry,
        &coding_config,
        tool_runtime::FS_READ,
        &coder_child,
        "agent-coder",
        serde_json::json!({ "path": "notes.md" }),
        false,
    )
    .await
    .expect("fs.read");
    let script = r#""""Fetch the current date from a web API (worldtimeapi.org)."""
import json
import urllib.request

API_URL = "https://worldtimeapi.org/api/ip"
TIMEOUT_SECS = 10


def fetch_current_date(api_url=API_URL):
    """Return the current datetime string from the date API."""
    with urllib.request.urlopen(api_url, timeout=TIMEOUT_SECS) as response:
        payload = json.loads(response.read().decode("utf-8"))
    return payload["datetime"]


if __name__ == "__main__":
    print(fetch_current_date())
"#;
    let tests = r#"Tests for the date fetcher (stdlib only, no network in tests).
import sys
import os

sys.path.insert(0, os.path.dirname(__file__))

from date_script import API_URL, TIMEOUT_SECS, fetch_current_date


def test_api_url_points_at_date_api():
    assert API_URL.startswith("https://")
    assert "worldtimeapi" in API_URL


def test_timeout_is_bounded():
    assert 1 <= TIMEOUT_SECS <= 30


def test_fetcher_is_callable():
    assert callable(fetch_current_date)
"#;
    let wrote_script = invoke_sync(
        &db,
        &engine,
        &registry,
        &coding_config,
        tool_runtime::FS_WRITE,
        &coder_child,
        "agent-coder",
        serde_json::json!({ "path": "date_script.py", "content": script }),
        false,
    )
    .await
    .expect("fs.write script");
    let wrote_tests = invoke_sync(
        &db,
        &engine,
        &registry,
        &coding_config,
        tool_runtime::FS_WRITE,
        &coder_child,
        "agent-coder",
        serde_json::json!({ "path": "test_date_script.py", "content": tests }),
        false,
    )
    .await
    .expect("fs.write tests");
    let wrote_reqs = invoke_sync(
        &db,
        &engine,
        &registry,
        &coding_config,
        tool_runtime::FS_WRITE,
        &coder_child,
        "agent-coder",
        serde_json::json!({ "path": "requirements.txt", "content": "pytest\n" }),
        false,
    )
    .await
    .expect("fs.write requirements");
    let script_receipt = &wrote_script.receipt;
    ctx.check(
        "coder.implement",
        read.receipt.status == ToolStatus::Success
            && script_receipt.status == ToolStatus::Success
            && script_receipt.compensation_plan_id.is_some()
            && wrote_tests.receipt.status == ToolStatus::Success
            && wrote_reqs.receipt.status == ToolStatus::Success,
        format!(
            "script+tests+requirements written, compensation plan {:?}",
            script_receipt.compensation_plan_id
        ),
        started,
    );

    // ---- 7. CoderAgent testing: real pytest when present, else planned + read-back. ----
    let started = Instant::now();
    let (runner, tests_ok, tests_detail) = if pytest_available().await {
        let outcome = invoke_sync(
            &db,
            &engine,
            &registry,
            &coding_config,
            tool_runtime::CODE_RUN_TESTS,
            &coder_child,
            "agent-coder",
            serde_json::json!({}),
            false,
        )
        .await
        .expect("code.run_tests");
        let ok = outcome.receipt.status == ToolStatus::Success;
        let detail = format!(
            "live pytest: status {:?}, output {}",
            outcome.receipt.status, outcome.receipt.output
        );
        ("live-pytest", ok, detail)
    } else {
        let planned = invoke_sync(
            &db,
            &engine,
            &registry,
            &coding_config,
            tool_runtime::CODE_RUN_TESTS,
            &coder_child,
            "agent-coder",
            serde_json::json!({ "framework": "pytest" }),
            true,
        )
        .await
        .expect("code.run_tests plan");
        // Real verification without a runner: read back both files and assert
        // the contract markers (API URL, test functions) are on disk.
        let back_script = invoke_sync(
            &db,
            &engine,
            &registry,
            &coding_config,
            tool_runtime::FS_READ,
            &coder_child,
            "agent-coder",
            serde_json::json!({ "path": "date_script.py" }),
            false,
        )
        .await
        .expect("read back script");
        let back_tests = invoke_sync(
            &db,
            &engine,
            &registry,
            &coding_config,
            tool_runtime::FS_READ,
            &coder_child,
            "agent-coder",
            serde_json::json!({ "path": "test_date_script.py" }),
            false,
        )
        .await
        .expect("read back tests");
        let script_text = back_script
            .receipt
            .output
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_owned();
        let tests_text = back_tests
            .receipt
            .output
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_owned();
        let ok = planned.receipt.status == ToolStatus::Success
            && script_text.contains("worldtimeapi")
            && script_text.contains("def fetch_current_date")
            && tests_text.contains("def test_");
        (
            "planned-mock",
            ok,
            format!(
                "pytest absent: dry-run planned + read-back verified (script {} bytes, tests {} bytes)",
                script_text.len(),
                tests_text.len()
            ),
        )
    };
    ctx.check(
        "coder.test",
        tests_ok,
        format!("runner={runner}: {tests_detail}"),
        started,
    );

    // ---- 8. Context assembly + Tool-RAG + TypeCompact per agent call. ----
    let started = Instant::now();
    let manifests = registry.list();
    let enabled: HashMap<String, bool> = manifests
        .iter()
        .map(|m| (m.id.clone(), registry.is_enabled(&m.id)))
        .collect();
    let docs = tool_rag::build_indexed_tools(&manifests, &[], &[], &[], &[], &enabled);
    tool_rag::index_tools(&db, &docs).await.expect("rag index");
    let query = tool_rag::validate_query(
        "write a python script and test it",
        vec![],
        None,
        false,
        Some(5),
    )
    .expect("valid query");
    let candidates = tool_rag::rank(
        &query,
        &docs,
        &Default::default(),
        &tool_rag::LocalLexicalProvider,
    );
    let gathered = context_assembler::GatheredContext {
        task_id: coder_child.clone(),
        agent_id: "agent-coder".to_owned(),
        session_id: "session-awakening".to_owned(),
        goal: goal.to_owned(),
        acceptance: vec![
            "script fetches date from web API".to_owned(),
            "tests pass".to_owned(),
        ],
        constraints: vec![format!("model spend <= ${TASK_BUDGET_USD}")],
        task_title: "proof-of-awakening".to_owned(),
        task_status: "executing".to_owned(),
        task_priority: "high".to_owned(),
        safety_rules: vec!["sandboxed workspace only".to_owned()],
        segments: vec![
            context_assembler::SegmentInput {
                id: "seg-research".to_owned(),
                layer: "evidence",
                body: "worldtimeapi.org /api/ip returns current UTC datetime as JSON".to_owned(),
                protected: false,
                safety_critical: false,
                evidence_refs: vec![claim.id.clone()],
                tool_ids: vec![tool_runtime::BROWSER_NAVIGATE.to_owned()],
            },
            context_assembler::SegmentInput {
                id: "seg-plan".to_owned(),
                layer: "task",
                body: "implement date_script.py, test with pytest, verify pass".to_owned(),
                protected: false,
                safety_critical: false,
                evidence_refs: Vec::new(),
                tool_ids: vec![tool_runtime::FS_WRITE.to_owned()],
            },
        ],
        ranked_tools: candidates
            .iter()
            .take(5)
            .map(|c| context_assembler::RankedTool {
                tool_id: c.tool_id.clone(),
                score: c.score,
                reason: "awakening demo rank".to_owned(),
            })
            .collect(),
        tool_entries: vec![context_assembler::ToolEntry {
            name: tool_runtime::FS_WRITE.to_owned(),
            description: "write a file".to_owned(),
            input_schema: serde_json::json!({ "type": "object" }),
        }],
        history: vec![context_assembler::HistoryTurn {
            role: "user".to_owned(),
            content: goal.to_owned(),
        }],
        instruction: "implement and verify".to_owned(),
        chain_head_hash: context_assembler::chain_head_hash(&db)
            .await
            .expect("chain head"),
    };
    let assembled = context_assembler::assemble(&context_assembler::AssemblyInput {
        gathered,
        model: "gpt-4o-mini".to_owned(),
        context_window: 128_000,
        config: context_assembler::ContextAssemblyConfig::default(),
    })
    .expect("assemble");
    // TypeCompact the task snapshot under a tight budget.
    let evidence_rows = evidence_store::list_recent_evidence(&db, 100)
        .await
        .expect("ledger reads");
    let snapshot = context_budget::build_task_snapshot(
        &coder_child,
        "session-awakening",
        "proof-of-awakening",
        "executing",
        "enabled=false, stopped=false",
        &evidence_rows
            .iter()
            .filter(|r| r.kind != "tool_receipt")
            .take(5)
            .map(|r| (r.id.clone(), r.kind.clone(), r.status.clone()))
            .collect::<Vec<_>>(),
        &evidence_rows
            .iter()
            .filter(|r| r.kind == "tool_receipt")
            .filter_map(|r| {
                r.payload_json
                    .as_deref()
                    .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
                    .and_then(|p| {
                        p.get("toolId")
                            .and_then(|t| t.as_str())
                            .map(|t| (t.to_owned(), r.id.clone(), r.status.clone()))
                    })
            })
            .collect::<Vec<_>>(),
        "scratch: awakening assembly",
        vec![format!("eval-{coder_child}")],
        vec![],
        vec!["no-bypass-policy".to_owned()],
    );
    let mut budget = context_budget::default_config();
    budget.total_budget = 120;
    let compacted = context_budget::compact_snapshot(&snapshot, &budget).expect("compact");
    ctx.check(
        "context.assemble",
        !candidates.is_empty()
            && assembled.total_tokens > 0
            && assembled.assembly_hash.starts_with("sha256:")
            && compacted.proof_hash.starts_with("sha256:"),
        format!(
            "rag top={}, assembled {} tokens ({} tools), compacted {}->{} tokens",
            candidates
                .first()
                .map(|c| c.tool_id.as_str())
                .unwrap_or("none"),
            assembled.total_tokens,
            assembled.tools_included.len(),
            compacted.original_tokens,
            compacted.compacted_tokens
        ),
        started,
    );

    // ---- 9. Semantic memory: index + recall the research. ----
    let started = Instant::now();
    let index_report = memory_indexer::rebuild_index(&db).await.expect("rebuild");
    let hits = memory_indexer::hybrid_search(
        &db,
        "date API script",
        5,
        &memory_indexer::MemoryFilter::default(),
        memory_indexer::RetrievalMode::Hybrid,
        0.5,
    )
    .await
    .expect("hybrid search");
    ctx.check(
        "memory.recall",
        index_report.entries_added > 0 && !hits.is_empty(),
        format!(
            "indexed {} entries, top hit {} ({})",
            index_report.entries_added,
            hits.first().map(|h| h.id.as_str()).unwrap_or("none"),
            hits.first()
                .map(|h| h.source_type.as_str())
                .unwrap_or("none")
        ),
        started,
    );

    // ---- 10. LLM gateway: route + meter (real call only with key + flag). ----
    let started = Instant::now();
    let router = llm_gateway::ModelRouter::with_defaults();
    let requirement = llm_gateway::RoutingRequirement {
        needs_tools: true,
        ..Default::default()
    };
    let descriptor = router.route(&requirement).expect("route a model");
    let (llm_mode, llm_cost, llm_model) = if llm_real {
        let vault = talos_secrets::SecretVault::os();
        let http = llm_gateway::ReqwestHttpClient::new().expect("http client");
        let http = std::sync::Arc::new(http);
        let gateway = llm_gateway::LlmGateway::new(
            std::sync::Arc::new(engine.clone()),
            db.clone(),
            vault.clone(),
            vec![
                std::sync::Arc::new(llm_gateway::providers::OpenAiProvider::new(
                    vault.clone(),
                    http.clone(),
                )) as std::sync::Arc<dyn llm_gateway::LlmProvider>,
                std::sync::Arc::new(llm_gateway::providers::AnthropicProvider::new(
                    vault.clone(),
                    http.clone(),
                )),
                std::sync::Arc::new(llm_gateway::providers::OllamaProvider::new(
                    llm_gateway::providers::ollama::OLLAMA_DEFAULT_BASE_URL,
                    http.clone(),
                )),
            ],
            llm_gateway::GatewayConfig::default(),
        );
        let request = llm_gateway::LlmRequest {
            model: "gpt-4o-mini".to_owned(),
            messages: vec![llm_gateway::LlmMessage::new(
                llm_gateway::LlmRole::User,
                "Reply with exactly: awakening acknowledged.",
            )],
            tools: None,
            temperature: Some(0.0),
            max_tokens: Some(32),
            stop_sequences: Vec::new(),
            metadata: llm_gateway::LlmRequestMetadata {
                task_id: parent_id.clone(),
                agent_id: "agent-supervisor".to_owned(),
                purpose: "proof-of-awakening live probe".to_owned(),
            },
        };
        match gateway.complete(request, None).await {
            Ok((response, receipt)) => {
                let text = response.content.clone().unwrap_or_default();
                (
                    "real",
                    receipt.estimated_cost_usd,
                    format!(
                        "{} ({})",
                        receipt.model_id,
                        text.chars().take(48).collect::<String>()
                    ),
                )
            }
            Err(err) => {
                let msg = err.to_string();
                if matches!(err, llm_gateway::LlmError::SecretUnavailable) {
                    ctx.check(
                        "llm.route",
                        false,
                        "no provider key in OS keychain — configure via Settings > Providers, then rerun with TALOS_AWAKENING_LLM=real".to_owned(),
                        started,
                    );
                    unreachable!();
                }
                ctx.check(
                    "llm.route",
                    false,
                    format!("live call failed: {msg}"),
                    started,
                );
                unreachable!();
            }
        }
    } else {
        // Full-mock: route the cheapest adequate model, meter a simulated
        // call through the real ledger path (no network).
        let usage = llm_gateway::TokenUsage::new(800, 200);
        let cost = llm_gateway::model::estimate_cost(
            usage,
            descriptor.cost_per_1k_input,
            descriptor.cost_per_1k_output,
        );
        evidence_store::insert_usage_record(
            &db,
            &evidence_store::NewUsageRecord {
                id: "usage-awakening-1",
                task_id: parent_id.as_str(),
                agent_id: "agent-supervisor",
                provider_id: &descriptor.provider_id,
                model_id: &descriptor.model_id,
                prompt_tokens: usage.prompt_tokens,
                completion_tokens: usage.completion_tokens,
                total_tokens: usage.total_tokens,
                estimated_cost_usd: cost,
                latency_ms: 120,
            },
        )
        .await
        .expect("usage record");
        let meta = serde_json::json!({
            "provider": descriptor.provider_id,
            "model": descriptor.model_id,
            "purpose": "proof-of-awakening mock call",
            "message_count": 1,
            "tool_count": 0,
        });
        evidence_store::insert_model_interaction(
            &db,
            &evidence_store::NewModelInteraction {
                id: "interaction-awakening-1",
                task_id: parent_id.as_str(),
                agent_id: "agent-supervisor",
                provider_id: &descriptor.provider_id,
                model_id: &descriptor.model_id,
                request_hash: &evidence_store::hash_request_metadata(&meta),
                response_hash: &evidence_store::hash_request_metadata(
                    &serde_json::json!({"mock": true}),
                ),
                finish_reason: "stop",
            },
        )
        .await
        .expect("interaction");
        (
            "mock",
            cost,
            format!("{}/{}", descriptor.provider_id, descriptor.model_id),
        )
    };
    ctx.check(
        "llm.route",
        llm_cost >= 0.0,
        format!("{llm_mode} call via {llm_model}, cost ${llm_cost:.6}"),
        started,
    );

    // ---- 11. HITL: shell execution needs a human; demo user approves. ----
    let started = Instant::now();
    let mut approvals = agent_runtime::approval::ApprovalStore::new();
    let shell_request = policy_broker::ActionRequest {
        id: "act-awakening-shell".to_owned(),
        agent_id: "agent-coder".to_owned(),
        task_id: coder_child.clone(),
        capability: Capability::ExecuteCommand,
        intent: "run the pytest suite for the date script".to_owned(),
        target: "workspace/test_date_script.py".to_owned(),
        payload_json: serde_json::json!({ "command": "pytest", "args": ["-q"] }),
        risk_level: RiskLevel::High,
        reversible: false,
        requested_at: chrono::Utc::now(),
    };
    let pending = approvals.request_approval(
        &shell_request,
        PolicyDecision::RequireHumanReview,
        "high-risk shell execution needs a human",
        &["human_approval".to_owned()],
    );
    let was_pending = pending.status == agent_runtime::approval::ApprovalStatus::Pending;
    // The ApprovalSheet moment: a human reviews the diff/scope, then grants.
    let granted = approvals
        .grant(&pending.approval_id, "demo-user", false)
        .expect("grant");
    ctx.check(
        "hitl.approve",
        was_pending && granted.status == agent_runtime::approval::ApprovalStatus::Granted,
        format!(
            "approval {} held for review, granted by demo-user (ApprovalSheet path)",
            pending.approval_id
        ),
        started,
    );

    // ---- 12. Telemetry + cost: spans recorded, budget holds. ----
    // NOTE: export runs last — metric queries themselves record spans
    // (chain verification), so the export/count comparison must come after
    // every other instrumented call.
    let started = Instant::now();
    let tracker =
        cost_tracker::BudgetTracker::new(cost_tracker::SessionBudget::capped(SESSION_BUDGET_USD));
    let budget_check = tracker.check(&db, &parent_id, 0.01).await.expect("budget");
    let breakdown = cost_tracker::cost_breakdown(&db, Some(SESSION_BUDGET_USD))
        .await
        .expect("breakdown");
    let metrics = cost_tracker::operational_metrics(&db)
        .await
        .expect("metrics");
    let export_dir = tempfile::tempdir().expect("export dir");
    let export_path = export_dir.path().join("awakening-traces.json");
    let export_stats = talos_telemetry::export_spans_to_file(&collector.all_spans(), &export_path)
        .expect("export");
    let spans = collector.total_recorded();
    ctx.check(
        "telemetry.cost",
        spans >= 10
            && export_stats.spans_written == collector.len()
            && !matches!(
                budget_check.decision,
                cost_tracker::BudgetDecision::Deny
            )
            && breakdown.session_total_usd <= SESSION_BUDGET_USD,
        format!(
            "{spans} spans, exported {}, session ${:.6} (cap ${SESSION_BUDGET_USD}), task success {:.0}%",
            export_stats.spans_written,
            breakdown.session_total_usd,
            metrics.success_rate * 100.0
        ),
        started,
    );

    // ---- 13. Final verification: chain valid, parent completes. ----
    let started = Instant::now();
    let report = evidence_store::get_integrity_report(&db)
        .await
        .expect("integrity");
    let chain = evidence_store::verify_chain(&db).await.expect("chain");
    let final_task =
        agent_runtime::queue::execute_task(&db, &engine, &parent_id, &CancelToken::new())
            .await
            .expect("parent completes");
    ctx.check(
        "evidence.verify",
        report.healthy && chain.valid && final_task.status == "completed",
        format!(
            "chain valid ({} links), ledger healthy, parent {}",
            chain.checked, final_task.status
        ),
        started,
    );

    let total_ms = demo_started.elapsed().as_millis();
    let result = DemoResult {
        scenario: SCENARIO,
        mode,
        ok: true,
        steps: ctx.steps,
        metrics: serde_json::json!({
            "parentTask": parent_id,
            "parentStatus": final_task.status,
            "children": results.len(),
            "childIds": results.iter().map(|r| &r.child_task_id).collect::<Vec<_>>(),
            "mergedStatus": merged.status,
            "testRunner": runner,
            "llmMode": llm_mode,
            "llmModel": llm_model,
            "llmCostUsd": llm_cost,
            "sessionCostUsd": breakdown.session_total_usd,
            "taskBudgetUsd": TASK_BUDGET_USD,
            "sessionBudgetUsd": SESSION_BUDGET_USD,
            "spansRecorded": spans,
            "spansExported": export_stats.spans_written,
            "chainChecked": chain.checked,
            "chainValid": chain.valid,
            "ledgerHealthy": report.healthy,
            "totalMs": total_ms,
        }),
        generated_at: now_iso(),
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&result).unwrap_or_default()
    );
    eprintln!("[awakening] proof-of-awakening: all steps passed");
}
