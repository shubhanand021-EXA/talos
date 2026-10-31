//! Batch 2 integration demo: one deterministic, local-only, policy-gated,
//! evidence-backed flow across every Batch 2 system.
//!
//! Scenario `batch2-integration-demo`:
//! 1. submit a task → runtime queues + executes it (`quick_success`)
//! 2. Tool-RAG ranks tools for a read intent → `fs.read` wins
//! 3. tool framework reads a seeded file live (policy allow, receipt kept)
//! 4. browser-use mock dry-runs navigate (planned, no mutation)
//! 5. computer-use mock dry-runs locate (bounding box, envelope disabled)
//! 6. context budget compacts the task snapshot (proof hash, invariants hold)
//! 7. fs.write live creates a `backup_restore` compensation plan
//! 8. evidence ledger holds every receipt; integrity report is healthy
//!
//! Prints a single JSON document to stdout (logs go to stderr). Exits
//! non-zero on the first failed expectation.

use std::collections::HashMap;

use agent_runtime::queue::{CancelToken, SubmitParams};
use policy_broker::DefaultPolicyEngine;
use talos_common::now_iso;
use tool_runtime::{ToolInvocation, ToolRegistry, ToolStatus};

#[derive(Debug, serde::Serialize)]
struct Step {
    name: &'static str,
    ok: bool,
    detail: String,
}

#[derive(Debug, serde::Serialize)]
struct DemoResult {
    scenario: &'static str,
    ok: bool,
    steps: Vec<Step>,
    metrics: serde_json::Value,
    generated_at: String,
}

fn step(steps: &mut Vec<Step>, name: &'static str, ok: bool, detail: String) {
    if !ok {
        eprintln!("[batch2-demo] FAILED: {name}: {detail}");
    }
    steps.push(Step { name, ok, detail });
}

fn fail(steps: &mut Vec<Step>, name: &'static str, detail: String) -> ! {
    step(steps, name, false, detail);
    let result = DemoResult {
        scenario: "batch2-integration-demo",
        ok: false,
        steps: std::mem::take(steps),
        metrics: serde_json::json!({}),
        generated_at: now_iso(),
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&result).unwrap_or_default()
    );
    std::process::exit(1);
}

fn check(steps: &mut Vec<Step>, name: &'static str, ok: bool, detail: String) {
    if ok {
        step(steps, name, true, detail);
    } else {
        fail(steps, name, detail);
    }
}

#[tokio::main]
async fn main() {
    let mut steps: Vec<Step> = Vec::new();

    // 1. Runtime: submit + execute.
    let db = evidence_store::EvidenceDb::connect_memory()
        .await
        .expect("in-memory db");
    let engine = DefaultPolicyEngine::new();
    let mut counter = 0_u64;
    let submitted = agent_runtime::queue::submit_task(
        &db,
        &mut counter,
        SubmitParams {
            title: "batch2-integration-demo".to_owned(),
            description: Some("Batch 2 end-to-end exercise".to_owned()),
            priority: "normal".to_owned(),
            timeout_ms: 30_000,
            max_attempts: 3,
            scenario: "quick_success".to_owned(),
        },
    )
    .await
    .expect("submit");
    check(
        &mut steps,
        "runtime.submit",
        submitted.status == "queued",
        format!("task {} queued", submitted.id),
    );
    let task_id = submitted.id.clone();
    let executed = agent_runtime::queue::execute_task(&db, &engine, &task_id, &CancelToken::new())
        .await
        .expect("execute");
    check(
        &mut steps,
        "runtime.execute",
        executed.status == "completed",
        format!("task {} completed", executed.id),
    );

    // Registry + sandbox (isolated temp root; computer envelope stays disabled:
    // dry-run only, exactly like the shipped default).
    let registry = ToolRegistry::with_builtins();
    let sandbox = tempfile::tempdir().expect("tempdir");
    let config = tool_runtime::SandboxConfig {
        root: sandbox.path().to_path_buf(),
        exec_enabled: false,
        exec_allowlist: Vec::new(),
        exec_timeout_ms: 5_000,
        max_bytes: 262_144,
        computer: computer_use::SafetyEnvelope::default(),
    };
    let invoke = |tool_id: &str, params: serde_json::Value, dry_run: bool| {
        tool_runtime::invoke(
            &db,
            &engine,
            &registry,
            &config,
            ToolInvocation {
                tool_id: tool_id.to_owned(),
                task_id: task_id.clone(),
                agent_id: "agent-demo".to_owned(),
                params,
                dry_run,
            },
        )
    };

    // 2. Tool-RAG suggests a safe tool for a read intent.
    let manifests = registry.list();
    let enabled: HashMap<String, bool> = manifests
        .iter()
        .map(|m| (m.id.clone(), registry.is_enabled(&m.id)))
        .collect();
    let docs = tool_rag::build_indexed_tools(&manifests, &[], &[], &[], &[], &enabled);
    tool_rag::index_tools(&db, &docs).await.expect("rag index");
    let query = tool_rag::validate_query("read a demo file", vec![], None, false, Some(5))
        .expect("valid query");
    let candidates = tool_rag::rank(
        &query,
        &docs,
        &Default::default(),
        &tool_rag::LocalLexicalProvider,
    );
    let top = candidates.first().expect("at least one candidate");
    check(
        &mut steps,
        "toolrag.suggest",
        top.tool_id == "fs.read",
        format!("top candidate {} score {}", top.tool_id, top.score),
    );

    // 3. Tool framework: live read of a seeded file (Low risk → allow).
    tokio::fs::write(sandbox.path().join("demo.txt"), b"hello batch2\n")
        .await
        .expect("seed");
    let read = invoke("fs.read", serde_json::json!({ "path": "demo.txt" }), false)
        .await
        .expect("fs.read")
        .receipt;
    check(
        &mut steps,
        "tools.fs_read",
        read.status == ToolStatus::Success && read.evidence_id.is_some(),
        format!(
            "policy {:?}, evidence {:?}",
            read.policy_decision, read.evidence_id
        ),
    );

    // 4. Browser-use mock dry-run: navigate plans without mutating.
    let nav = invoke(
        "browser.navigate",
        serde_json::json!({ "url": "https://example.com" }),
        true,
    )
    .await
    .expect("browser.navigate")
    .receipt;
    check(
        &mut steps,
        "browser.navigate_dryrun",
        nav.status == ToolStatus::Success
            && nav.dry_run
            && nav.output.get("planned") == Some(&serde_json::json!(true))
            && nav.output.get("domHash").is_some(),
        format!(
            "domHash {}",
            nav.output
                .get("domHash")
                .unwrap_or(&serde_json::Value::Null)
        ),
    );

    // 5. Computer-use mock dry-run: locate resolves a bounding box.
    let locate = invoke(
        "computer.locate_element",
        serde_json::json!({ "selector": "button#ok" }),
        true,
    )
    .await
    .expect("computer.locate_element")
    .receipt;
    check(
        &mut steps,
        "computer.locate_dryrun",
        locate.status == ToolStatus::Success
            && locate
                .output
                .get("boundingBox")
                .is_some_and(|b| b.is_object()),
        format!(
            "boundingBox {}",
            locate
                .output
                .get("boundingBox")
                .unwrap_or(&serde_json::Value::Null)
        ),
    );

    // 6. Context budget: compact the task snapshot under pressure.
    let evidence_rows = evidence_store::list_recent_evidence(&db, 100)
        .await
        .expect("ledger reads");
    let snapshot = context_budget::build_task_snapshot(
        &task_id,
        "session-local",
        "batch2-integration-demo",
        "completed",
        "enabled=false, stopped=false, dry-run default",
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
        "scratch: verify every Batch 2 layer links policy to evidence",
        vec![format!("eval-{task_id}")],
        vec![],
        vec!["no-bypass-policy".to_owned()],
    );
    let mut budget = context_budget::default_config();
    budget.total_budget = 120;
    let compacted = context_budget::compact_snapshot(&snapshot, &budget).expect("compact");
    check(
        &mut steps,
        "context.compact",
        compacted.proof_hash.starts_with("sha256:")
            && !compacted.preserved_invariants.is_empty()
            && compacted.compacted_tokens <= compacted.original_tokens,
        format!(
            "{} -> {} tokens, proof {}",
            compacted.original_tokens, compacted.compacted_tokens, compacted.proof_hash
        ),
    );

    // 7. Compensation: fs.write is evidence-gated (read receipt above
    // satisfies it) and creates a backup_restore plan before mutating.
    let wrote = invoke(
        "fs.write",
        serde_json::json!({ "path": "out.txt", "content": "batch2 ok" }),
        false,
    )
    .await
    .expect("fs.write")
    .receipt;
    check(
        &mut steps,
        "tools.fs_write_compensated",
        wrote.status == ToolStatus::Success && wrote.compensation_plan_id.is_some(),
        format!("plan {:?}", wrote.compensation_plan_id),
    );

    // 8. Ledger holds every receipt; integrity chain verifies.
    let rows = evidence_store::list_recent_evidence(&db, 100)
        .await
        .expect("ledger reads");
    let receipt_rows = rows.iter().filter(|r| r.kind == "tool_receipt").count();
    let report = evidence_store::get_integrity_report(&db)
        .await
        .expect("integrity");
    check(
        &mut steps,
        "evidence.ledger",
        receipt_rows >= 4 && report.healthy,
        format!(
            "{receipt_rows} tool_receipt rows, chain healthy={}",
            report.healthy
        ),
    );

    let result = DemoResult {
        scenario: "batch2-integration-demo",
        ok: true,
        steps,
        metrics: serde_json::json!({
            "taskId": task_id,
            "receipts": receipt_rows,
            "evidenceRows": rows.len(),
            "topCandidate": top.tool_id,
            "topScore": top.score,
            "originalTokens": compacted.original_tokens,
            "compactedTokens": compacted.compacted_tokens,
            "proofHash": compacted.proof_hash,
            "compensationPlan": wrote.compensation_plan_id,
            "taskStatus": executed.status,
        }),
        generated_at: now_iso(),
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&result).unwrap_or_default()
    );
}
