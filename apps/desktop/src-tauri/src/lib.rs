//! Talos desktop shell: Tauri 2 container for the React command center.
//!
//! The shell owns one [`AppState`](crate::state::AppState) (policy engine +
//! evidence store + agent runtime + tool registry + computer-use safety
//! envelope + Tool-RAG index tables + LLM gateway + assembly config +
//! per-task coding workspaces + semantic memory index) and exposes
//! thirty-nine least-privilege
//! IPC commands. A 5s
//! heartbeat keeps the frontend clock honest. See
//! `apps/desktop/README.md` for the command surface and security notes.

mod approval;
mod assembly;
mod commands;
mod delegation;
mod events;
pub(crate) mod llm;
mod memory;
mod state;
pub(crate) mod telemetry;
mod workspace;

use std::sync::atomic::{AtomicU64, Ordering};

use tauri::{Emitter, Manager};

use state::AppState;

use crate::events::HEARTBEAT;

/// Tauri entry point: build state, register commands, run the main window.
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let state = tauri::async_runtime::block_on(AppState::new_in_memory()).map_err(
                |err| -> Box<dyn std::error::Error> {
                    eprintln!("[talos] failed to initialize state: {err}");
                    Box::new(std::io::Error::other(err.to_string()))
                },
            )?;
            let started_at = state.started_at;
            app.manage(state);
            start_heartbeat(app.handle().clone(), started_at);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_info,
            commands::get_system_status,
            commands::request_action,
            commands::run_diagnostic,
            commands::verify_evidence_ledger,
            commands::submit_task,
            commands::cancel_task,
            commands::retry_task,
            commands::list_tasks,
            commands::get_task_state,
            commands::list_tools,
            commands::describe_tool,
            commands::invoke_tool_dry_run,
            commands::invoke_tool,
            commands::computer_safety_status,
            commands::computer_emergency_stop,
            commands::computer_reset_stop,
            commands::computer_session_scope,
            commands::computer_set_session_scope,
            commands::context_budget_status,
            commands::context_compact,
            commands::context_diagnostic_snapshot,
            commands::query_tool_candidates,
            llm::list_providers,
            llm::list_models,
            llm::test_provider_connection,
            llm::get_usage_summary,
            llm::store_provider_key,
            llm::delete_provider_key,
            assembly::preview_context,
            assembly::get_context_config,
            assembly::update_context_config,
            delegation::delegate_task,
            delegation::get_task_tree,
            delegation::cancel_subtree,
            workspace::create_workspace,
            workspace::get_workspace,
            workspace::list_workspaces,
            workspace::cleanup_workspace,
            workspace::get_workspace_tree,
            memory::search_memory,
            memory::get_index_status,
            memory::rebuild_index,
            memory::list_skills,
            approval::get_pending_approvals,
            approval::request_approval_review,
            approval::grant_approval,
            approval::deny_approval,
            approval::modify_approval,
            approval::batch_approve_low_risk,
            approval::set_task_mode,
            approval::get_task_mode,
            approval::get_plan_document,
            approval::execute_compensation,
            approval::fail_compensation_step,
            telemetry::get_operational_metrics,
            telemetry::get_cost_breakdown,
            telemetry::get_trace_summary,
            telemetry::export_telemetry,
            telemetry::set_session_budget,
        ])
        .run(tauri::generate_context!())
        .unwrap_or_else(|err| {
            eprintln!("[talos] failed to run application: {err}");
        });
}

/// Emit a `heartbeat` event every 5s so the UI clock and link state stay live.
fn start_heartbeat(app: tauri::AppHandle, started_at: std::time::Instant) {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(5));
        loop {
            ticker.tick().await;
            let payload = serde_json::json!({
                "source": "talos-desktop",
                "timestamp": talos_common::now_iso(),
                "sequence": SEQUENCE.fetch_add(1, Ordering::Relaxed),
                "uptimeSeconds": started_at.elapsed().as_secs_f64(),
            });
            if let Err(err) = app.emit(HEARTBEAT, payload) {
                eprintln!("[talos] heartbeat emission failed: {err}");
            }
        }
    });
}
