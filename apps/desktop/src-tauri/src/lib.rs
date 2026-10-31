//! Talos desktop shell: Tauri 2 container for the React command center.
//!
//! The shell owns one [`AppState`](crate::state::AppState) (policy engine +
//! evidence store) and exposes five least-privilege IPC commands. See
//! `apps/desktop/README.md` for the command surface and security notes.

mod commands;
mod events;
mod state;

use tauri::Manager;

use state::AppState;

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
            app.manage(state);
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
