//! One-line `tracing` bootstrap shared by Talos binaries and tests.

use tracing_subscriber::EnvFilter;

use crate::error::{Result, TalosError};

/// Install a global `tracing` subscriber (idempotent: second calls are no-ops).
pub fn init_tracing() -> Result<()> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let result = tracing_subscriber::fmt().with_env_filter(filter).try_init();
    match result {
        Ok(()) => Ok(()),
        Err(err) => {
            // `try_init` fails when a global subscriber is already installed
            // (e.g. multiple tests in one process). That state is usable.
            let message = err.to_string();
            if message.contains("already") || message.contains("installed") {
                Ok(())
            } else {
                Err(TalosError::Internal(message))
            }
        }
    }
}
