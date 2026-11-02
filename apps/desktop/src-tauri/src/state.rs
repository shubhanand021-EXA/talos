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
    /// OS-keychain vault for BYOK provider keys (values never leave it
    /// except into an HTTP `Authorization` header).
    pub secrets: talos_secrets::SecretVault,
    /// Policy-gated LLM gateway (engine + evidence shared by handle).
    pub llm: tokio::sync::Mutex<llm_gateway::LlmGateway>,
    /// Context assembly budget configuration (validated percentages).
    pub context_config: tokio::sync::Mutex<context_assembler::ContextAssemblyConfig>,
    /// Computer-use safety envelope (global enable flag off by default,
    /// emergency stop latch, rate limit, bounds, dangerous zones).
    /// Shared by handle: interior-mutable, no outer lock needed.
    pub computer: computer_use::SafetyEnvelope,
    /// In-memory OpenTelemetry-compatible span collector (local-first:
    /// buffered spans, JSON file export only, 7-day retention).
    pub telemetry: talos_telemetry::SpanCollector,
    /// Directory for explicit `export_telemetry` JSON files.
    pub telemetry_export_dir: std::path::PathBuf,
    /// Global session spend cap in USD (`None` = uncapped metering only).
    pub session_budget_usd: tokio::sync::Mutex<Option<f64>>,
    /// Sandbox root confining every tool filesystem path.
    pub tool_root: std::path::PathBuf,
    /// Per-task isolated coding workspaces (git worktree or temp dir).
    /// Workspace-backed tasks execute with the workspace root as their only
    /// filesystem root, the coder allowlist, and network-gated installs.
    pub workspaces: tokio::sync::Mutex<workspace_manager::WorkspaceManager>,
    /// Human-in-the-loop approvals (Plan/Act modes, queue, compensation).
    /// Guarded: commands share `&AppState`.
    pub approvals: tokio::sync::Mutex<agent_runtime::approval::ApprovalStore>,
    /// Supervisor tuning (fan-out/depth enforced per spawn, TAL-005).
    /// Runtime-configurable via `update_supervisor_config`.
    pub supervisor_config: tokio::sync::Mutex<agent_runtime::supervisor::SupervisorConfig>,
    /// Planning settings (model, timeout, fallback behavior).
    /// Runtime-configurable via `update_planning_config`.
    pub planning_config: tokio::sync::Mutex<agent_runtime::planner::PlanningConfig>,
    /// Process execution master switch (default false).
    pub exec_enabled: bool,
    /// Allowlisted executables when `exec_enabled`.
    pub exec_allowlist: Vec<String>,
    /// Process launch time (for uptime reporting).
    pub started_at: std::time::Instant,
    /// Static application metadata.
    pub app_info: AppInfo,
    /// File-backed database path (`None` for in-memory test state).
    pub db_path: Option<std::path::PathBuf>,
    /// Backup directory (`None` for in-memory test state).
    pub backups_dir: Option<std::path::PathBuf>,
    /// Startup crash-recovery outcome (`None` for in-memory test state,
    /// which never needs recovery).
    pub startup_recovery: Option<evidence_store::RecoveryReport>,
    /// Whether the evidence database is encrypted at rest.
    pub encryption_enabled: bool,
    /// Loud fallback reason when the database runs unencrypted
    /// (keychain unavailable). `None` when encrypted.
    pub encryption_warning: Option<String>,
}

impl AppState {
    /// Build state backed by an isolated in-memory database.
    ///
    /// Tests only: production must use [`AppState::new_persistent`] so state
    /// survives process restarts. Anything written here vanishes on drop.
    #[cfg(test)]
    pub async fn new_in_memory() -> Result<Self> {
        Self::new_in_memory_with_vault(talos_secrets::SecretVault::os()).await
    }

    /// Build state with an explicit secret vault (tests inject the mock).
    #[cfg(test)]
    pub async fn new_in_memory_with_vault(vault: talos_secrets::SecretVault) -> Result<Self> {
        let db = evidence_store::EvidenceDb::connect_memory()
            .await
            .map_err(|err: TalosError| err)?;
        Self::build(vault, db, None, None, None, false, None).await
    }

    /// Build state backed by the durable file database under `app_data_dir`.
    ///
    /// Production entry point: resolves `<app_data_dir>/talos.db` (creating
    /// directories as needed), unlocks or creates the OS-keychain database
    /// key, opens the database encrypted at rest (plaintext fallback only
    /// when the keychain is unavailable, with a loud warning), runs startup
    /// crash recovery, and logs the resolved path (path only, never content).
    pub async fn new_persistent(app_data_dir: &std::path::Path) -> Result<Self> {
        Self::new_persistent_with_vault(app_data_dir, talos_secrets::SecretVault::os()).await
    }

    /// Build durable state with an explicit secret vault.
    pub async fn new_persistent_with_vault(
        app_data_dir: &std::path::Path,
        vault: talos_secrets::SecretVault,
    ) -> Result<Self> {
        let layout = evidence_store::storage::ensure_layout(app_data_dir)?;
        eprintln!("[talos] data directory: {}", layout.base.display());
        // At-rest encryption: 256-bit key from the OS keychain. A missing or
        // unreachable keychain falls back to plaintext with a loud warning
        // (documented limitation, never silent).
        let (db, encryption_enabled, encryption_warning) =
            match evidence_store::load_or_create_key(&vault) {
                Ok(evidence_store::KeyState::Encrypted(key)) => {
                    let db =
                        evidence_store::EvidenceDb::connect_encrypted_file(&layout.db_file, &key)
                            .await?;
                    eprintln!("[talos] database: encrypted at rest (ChaCha20-Poly1305 envelope)");
                    let _ = evidence_store::append_event(
                        &db,
                        evidence_store::NewEvent {
                            id: format!("encryption-enabled-{}", uuid::Uuid::new_v4()),
                            kind: "encryption_enabled".to_owned(),
                            payload_json: serde_json::json!({
                                "method": "chacha20poly1305-envelope",
                                "key_source": "os-keychain",
                            }),
                        },
                    )
                    .await;
                    (db, true, None)
                }
                Ok(evidence_store::KeyState::Unavailable(reason)) => {
                    let warning =
                        format!("database keychain unavailable ({reason}); RUNNING UNENCRYPTED");
                    eprintln!("[talos] WARNING: {warning}");
                    let db = evidence_store::EvidenceDb::connect_file(&layout.db_file).await?;
                    let _ = evidence_store::append_event(
                        &db,
                        evidence_store::NewEvent {
                            id: format!("encryption-unavailable-{}", uuid::Uuid::new_v4()),
                            kind: "encryption_unavailable".to_owned(),
                            payload_json: serde_json::json!({ "reason": reason }),
                        },
                    )
                    .await;
                    (db, false, Some(warning))
                }
                Err(err) => return Err(err),
            };
        eprintln!(
            "[talos] database: {} (journal_mode={})",
            layout.db_file.display(),
            db.journal_mode()
                .await
                .unwrap_or_else(|_| "unknown".to_owned())
        );
        let recovery = evidence_store::run_startup_recovery(&db).await?;
        eprintln!(
            "[talos] startup recovery: chain_valid={} recovered={} dirty_shutdown={}",
            recovery.chain_valid,
            recovery.recovered.len(),
            recovery.dirty_shutdown,
        );
        if recovery.dirty_shutdown {
            eprintln!("[talos] WARNING: previous shutdown was unclean; crash recovery ran");
        }
        for task in &recovery.recovered {
            if task.reason == "resumed_from_checkpoint" {
                eprintln!("[talos] resumed task {} from checkpoint", task.task_id);
            }
        }
        Self::build(
            vault,
            db,
            Some(layout.db_file),
            Some(layout.backups_dir),
            Some(recovery),
            encryption_enabled,
            encryption_warning,
        )
        .await
    }

    /// Shared constructor: all state except the evidence database.
    #[allow(clippy::too_many_arguments)]
    async fn build(
        vault: talos_secrets::SecretVault,
        db: evidence_store::EvidenceDb,
        db_path: Option<std::path::PathBuf>,
        backups_dir: Option<std::path::PathBuf>,
        startup_recovery: Option<evidence_store::RecoveryReport>,
        encryption_enabled: bool,
        encryption_warning: Option<String>,
    ) -> Result<Self> {
        let tool_root = std::env::temp_dir().join("talos-tools");
        if let Err(err) = tokio::fs::create_dir_all(&tool_root).await {
            return Err(TalosError::Storage(format!("tool root: {err}")));
        }
        let workspace_base = std::env::temp_dir().join("talos-workspaces");
        if let Err(err) = tokio::fs::create_dir_all(&workspace_base).await {
            return Err(TalosError::Storage(format!("workspace base: {err}")));
        }
        let engine = DefaultPolicyEngine::new();
        let http: std::sync::Arc<dyn llm_gateway::HttpClient> = std::sync::Arc::new(
            llm_gateway::ReqwestHttpClient::new()
                .map_err(|err: String| TalosError::Internal(format!("http client: {err}")))?,
        );
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
        // Background semantic-memory indexer: incremental pass every 60s so
        // recall stays fresh without blocking IPC. Failures only log.
        {
            let index_db = db.clone();
            tokio::spawn(async move {
                let mut ticker = tokio::time::interval(std::time::Duration::from_secs(60));
                loop {
                    ticker.tick().await;
                    if let Err(err) = memory_indexer::index_incremental(&index_db).await {
                        eprintln!("[talos] background memory index failed: {err}");
                    }
                }
            });
        }
        Ok(Self {
            engine,
            db,
            runtime: tokio::sync::Mutex::new(LocalOrchestrator::new()),
            tools: tokio::sync::Mutex::new(tool_runtime::ToolRegistry::with_builtins()),
            secrets: vault,
            llm: tokio::sync::Mutex::new(gateway),
            context_config: tokio::sync::Mutex::new(
                context_assembler::ContextAssemblyConfig::default(),
            ),
            computer: computer_use::SafetyEnvelope::default(),
            telemetry: {
                let collector = talos_telemetry::SpanCollector::with_defaults();
                // Install the process-wide sink so every instrumented crate
                // records here without handle plumbing (first call wins).
                talos_telemetry::set_global_collector(collector.clone());
                collector
            },
            telemetry_export_dir: std::env::temp_dir().join("talos-traces"),
            session_budget_usd: tokio::sync::Mutex::new(None),
            tool_root,
            workspaces: tokio::sync::Mutex::new(workspace_manager::WorkspaceManager::new(
                workspace_base,
            )),
            approvals: tokio::sync::Mutex::new(agent_runtime::approval::ApprovalStore::new()),
            supervisor_config: tokio::sync::Mutex::new(
                agent_runtime::supervisor::SupervisorConfig::default(),
            ),
            planning_config: tokio::sync::Mutex::new(
                agent_runtime::planner::PlanningConfig::default(),
            ),
            exec_enabled: false,
            exec_allowlist: Vec::new(),
            started_at: std::time::Instant::now(),
            app_info: AppInfo {
                name: "Talos".to_owned(),
                version: env!("CARGO_PKG_VERSION").to_owned(),
                environment: "local".to_owned(),
            },
            db_path,
            backups_dir,
            startup_recovery,
            encryption_enabled,
            encryption_warning,
        })
    }
}
