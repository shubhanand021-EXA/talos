//! Shared desktop state: policy engine + evidence store + launch metadata.

use policy_broker::DefaultPolicyEngine;
use talos_common::{Result, TalosError};

/// Application metadata surfaced via `get_app_info`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AppInfo {
    /// Product name.
    pub name: String,
    /// Semantic version.
    pub version: String,
    /// Deployment environment.
    pub environment: String,
}

/// Tauri-managed state. Constructed once at startup, shared by all commands.
#[derive(Debug)]
pub struct AppState {
    /// Policy authority for every side-effecting action.
    pub engine: DefaultPolicyEngine,
    /// Append-only evidence database.
    pub db: evidence_store::EvidenceDb,
    /// Process launch time (for uptime reporting).
    pub started_at: std::time::Instant,
    /// Static application metadata.
    pub app_info: AppInfo,
}

impl AppState {
    /// Build state backed by an isolated in-memory database.
    ///
    /// Used at startup (deterministic, no filesystem scope needed) and in tests.
    pub async fn new_in_memory() -> Result<Self> {
        let db = evidence_store::EvidenceDb::connect_memory()
            .await
            .map_err(|err: TalosError| err)?;
        Ok(Self {
            engine: DefaultPolicyEngine::new(),
            db,
            started_at: std::time::Instant::now(),
            app_info: AppInfo {
                name: "Talos".to_owned(),
                version: env!("CARGO_PKG_VERSION").to_owned(),
                environment: "local".to_owned(),
            },
        })
    }
}
