//! Talos desktop shell: Tauri 2 container for the React command center.
//!
//! The shell owns one [`AppState`](crate::state::AppState) (policy engine +
//! evidence store + agent runtime + tool registry + computer-use safety
//! envelope + Tool-RAG index tables + LLM gateway + assembly config +
//! per-task coding workspaces + semantic memory index + MCP registry) and exposes
//! ninety-six least-privilege
//! IPC commands. A 5s
//! heartbeat keeps the frontend clock honest. See
//! `apps/desktop/README.md` for the command surface and security notes.

mod approval;
mod assembly;
mod commands;
mod delegation;
mod events;
pub(crate) mod gateway;
pub(crate) mod llm;
pub(crate) mod mcp;
mod memory;
pub(crate) mod sandbox;
mod skills;
mod state;
pub(crate) mod storage;
pub(crate) mod telemetry;
mod updater;
mod workspace;

use std::sync::atomic::{AtomicU64, Ordering};

use tauri::{Emitter, Manager, WindowEvent};

use state::AppState;

use crate::events::HEARTBEAT;
use crate::gateway::{GatewayClient, GatewayLink, HEARTBEAT_INTERVAL_MS};

/// Tauri entry point: build state, register commands, run the main window.
///
/// The shell is a **client** of the persistent `talos-gateway` daemon
/// (Batch 5): on startup it probes `ws://127.0.0.1:8080/talos`, spawns a
/// detached daemon when absent, and routes IPC through it. Closing this
/// window never stops the daemon — tasks and ledger writes continue.
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            // Durable storage: the Tauri app-data dir for `com.talos.desktop`
            // (platform default), falling back to the evidence_store resolver
            // when the path API is unavailable (tests, portable installs).
            let app_data = app
                .path()
                .app_data_dir()
                .or_else(|_| evidence_store::storage::resolve_app_data_dir())
                .map_err(|err| -> Box<dyn std::error::Error> {
                    eprintln!("[talos] failed to resolve app data dir: {err}");
                    Box::new(std::io::Error::other(err.to_string()))
                })?;
            let state = tauri::async_runtime::block_on(AppState::new_persistent(&app_data))
                .map_err(|err| -> Box<dyn std::error::Error> {
                    eprintln!("[talos] failed to initialize state: {err}");
                    Box::new(std::io::Error::other(err.to_string()))
                })?;
            let started_at = state.started_at;
            app.manage(state);
            // Gateway client: probe the daemon, spawn a detached child when
            // absent, and keep the link live with heartbeats. The daemon
            // survives this window's exit by design (persistent background
            // process holding the exclusive WAL lock).
            let gateway = GatewayClient::new();
            app.manage(gateway.clone());
            {
                let client = gateway.clone();
                tauri::async_runtime::spawn(async move {
                    let link = client.ensure_running().await;
                    eprintln!("[talos] gateway link: {link:?}");
                });
            }
            start_gateway_heartbeat(app.handle().clone(), gateway);
            // Daemon broadcast forwarder: scheduler firings + PolicyRequired
            // desktop notifications surface as `gatewayDaemonEvent` Tauri
            // events for the UI (short-lived command sockets miss broadcasts).
            gateway::forward_daemon_events(app.handle().clone());
            start_heartbeat(app.handle().clone(), started_at);
            Ok(())
        })
        .on_window_event(|app, event| {
            // Orderly shutdown: mark clean (suppresses next-boot crash
            // recovery) and seal any encrypted database before exit.
            if let WindowEvent::CloseRequested { .. } = event {
                let handle = app.clone();
                tauri::async_runtime::block_on(async move {
                    let state = handle.state::<AppState>();
                    if let Err(err) = state.db.close_encrypted().await {
                        eprintln!("[talos] shutdown seal failed: {err}");
                    }
                    if let Err(err) = evidence_store::mark_clean_shutdown(&state.db).await {
                        eprintln!("[talos] shutdown flag clear failed: {err}");
                    }
                });
            }
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
            sandbox::get_sandbox_status,
            sandbox::get_sandbox_profile_for_task,
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
            delegation::update_supervisor_config,
            delegation::preview_plan,
            delegation::get_planning_config,
            delegation::update_planning_config,
            workspace::create_workspace,
            workspace::get_workspace,
            workspace::list_workspaces,
            workspace::cleanup_workspace,
            workspace::get_workspace_tree,
            memory::search_memory,
            memory::get_index_status,
            memory::rebuild_index,
            memory::list_skills,
            skills::get_skill,
            skills::validate_skill,
            skills::publish_skill,
            skills::deprecate_skill,
            skills::archive_skill,
            skills::get_skill_stats,
            skills::list_skill_versions,
            skills::diff_skill_versions,
            skills::trigger_skill_evaluation,
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
            storage::get_storage_info,
            storage::get_onboarding_state,
            storage::set_onboarding_state,
            storage::create_backup,
            storage::restore_backup,
            storage::list_backups,
            updater::check_for_updates,
            updater::get_update_status,
            updater::download_update,
            updater::install_update,
            updater::verify_update_manifest,
            updater::get_update_channel,
            updater::set_update_channel,
            mcp::list_mcp_servers,
            mcp::add_mcp_server,
            mcp::remove_mcp_server,
            mcp::list_mcp_tools,
            mcp::call_mcp_tool,
            mcp::parse_instruction_file,
            mcp::list_plugins,
            mcp::approve_plugin,
            gateway::gateway_status,
            gateway::gateway_ping,
            gateway::gateway_restart,
            gateway::gateway_forward,
            gateway::list_schedules,
            gateway::toggle_schedule_active,
            gateway::trigger_schedule_now,
        ])
        .run(tauri::generate_context!())
        .unwrap_or_else(|err| {
            eprintln!("[talos] failed to run application: {err}");
        });
}

/// Gateway heartbeat: `Ping`/`Pong` every 5s. Three missed pongs flip the
/// link to `Disconnected` and emit `gatewayStatusChanged` so the UI renders
/// its DISCONNECTED banner with a Restart Daemon button.
fn start_gateway_heartbeat(app: tauri::AppHandle, client: GatewayClient) {
    tauri::async_runtime::spawn(async move {
        let mut ticker =
            tokio::time::interval(std::time::Duration::from_millis(HEARTBEAT_INTERVAL_MS));
        loop {
            ticker.tick().await;
            let ok = client.heartbeat().await;
            let status = client.status().await;
            if !ok && status.link == GatewayLink::Disconnected {
                if let Err(err) = app.emit("gatewayStatusChanged", &status) {
                    eprintln!("[talos] gateway status emission failed: {err}");
                }
            }
        }
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
