//! `talos-gateway`: persistent headless background daemon.
//!
//! Owns the evidence database (exclusive WAL lock + v0.4.0 encryption
//! envelope) and serves local clients over `ws://127.0.0.1:8080/talos`.
//! Survives desktop-window close by design: spawn once, run forever.
//!
//! ```sh
//! talos-gateway                          # production: platform data dir
//! TALOS_DATA_DIR=/tmp/talos-ci talos-gateway   # isolated ledger (CI/tests)
//! TALOS_GATEWAY_PORT=8080 talos-gateway  # explicit port (default 8080)
//! ```

use talos_gateway::{serve, GatewayState};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    if let Err(err) = run().await {
        eprintln!("[talos-gateway] fatal: {err}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    let port: u16 = std::env::var("TALOS_GATEWAY_PORT")
        .ok()
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(talos_gateway::GATEWAY_PORT);
    // Data dir: explicit override first (CI/tests), then the shared resolver
    // (platform app-data dir), so daemon and desktop never diverge on path.
    let data_dir = if let Ok(dir) = std::env::var("TALOS_DATA_DIR") {
        std::path::PathBuf::from(dir)
    } else {
        evidence_store::storage::resolve_app_data_dir()
            .map_err(|err| format!("resolve app data dir: {err}"))?
    };
    eprintln!("[talos-gateway] starting (port {port})");
    let state = GatewayState::new_persistent(&data_dir)
        .await
        .map_err(|err| format!("init state: {err}"))?;
    let _handle = serve(state, port)
        .await
        .map_err(|err| format!("serve: {err}"))?;
    // Park forever: the daemon only exits on signal or fatal error.
    // Shutdown sealing (WAL checkpoint + envelope re-encrypt) runs on SIGINT
    // via the state handle's Drop path in future work; for now Ctrl-C exits.
    tokio::signal::ctrl_c()
        .await
        .map_err(|err| format!("signal: {err}"))?;
    eprintln!("[talos-gateway] shutting down");
    Ok(())
}
