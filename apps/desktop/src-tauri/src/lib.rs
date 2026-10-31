//! Talos desktop shell: Tauri 2 container for the React command center.
//!
//! The shell owns one [`AppState`](crate::state::AppState) (policy engine +
//! evidence store + agent runtime) and exposes five least-privilege IPC
//! commands. A 5s heartbeat keeps the frontend clock honest. See
//! `apps/desktop/README.md` for the command surface and security notes.

mod commands;
mod events;
mod state;

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
