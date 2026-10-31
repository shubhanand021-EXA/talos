//! Shared desktop state: policy engine + evidence store + agent runtime +
//! tool registry.

use agent_runtime::LocalOrchestrator;
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
    /// Local orchestration runtime (guarded: commands share `&AppState`).
    pub runtime: tokio::sync::Mutex<LocalOrchestrator>,
    /// Tool registry (enablement flags; manifests are static).
    pub tools: tokio::sync::Mutex<tool_runtime::ToolRegistry>,
    /// Computer-use safety envelope (global enable flag off by default,
    /// emergency stop latch, rate limit, bounds, dangerous zones).
    /// Shared by handle: interior-mutable, no outer lock needed.
    pub computer: computer_use::SafetyEnvelope,
    /// Sandbox root confining every tool filesystem path.
    pub tool_root: std::path::PathBuf,
    /// Process execution master switch (default false).
    pub exec_enabled: bool,
    /// Allowlisted executables when `exec_enabled`.
    pub exec_allowlist: Vec<String>,
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
        let tool_root = std::env::temp_dir().join("talos-tools");
        if let Err(err) = tokio::fs::create_dir_all(&tool_root).await {
            return Err(TalosError::Storage(format!("tool root: {err}")));
        }
        Ok(Self {
            engine: DefaultPolicyEngine::new(),
            db,
            runtime: tokio::sync::Mutex::new(LocalOrchestrator::new()),
            tools: tokio::sync::Mutex::new(tool_runtime::ToolRegistry::with_builtins()),
            computer: computer_use::SafetyEnvelope::default(),
            tool_root,
            exec_enabled: false,
            exec_allowlist: Vec::new(),
            started_at: std::time::Instant::now(),
            app_info: AppInfo {
                name: "Talos".to_owned(),
                version: env!("CARGO_PKG_VERSION").to_owned(),
                environment: "local".to_owned(),
            },
        })
    }
}
