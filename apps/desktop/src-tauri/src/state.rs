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
    /// Sandbox root confining every tool filesystem path.
    pub tool_root: std::path::PathBuf,
    /// Per-task isolated coding workspaces (git worktree or temp dir).
    /// Workspace-backed tasks execute with the workspace root as their only
    /// filesystem root, the coder allowlist, and network-gated installs.
    pub workspaces: tokio::sync::Mutex<workspace_manager::WorkspaceManager>,
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
        Self::new_in_memory_with_vault(talos_secrets::SecretVault::os()).await
    }

    /// Build state with an explicit secret vault (tests inject the mock).
    pub async fn new_in_memory_with_vault(vault: talos_secrets::SecretVault) -> Result<Self> {
        let db = evidence_store::EvidenceDb::connect_memory()
            .await
            .map_err(|err: TalosError| err)?;
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
            tool_root,
            workspaces: tokio::sync::Mutex::new(workspace_manager::WorkspaceManager::new(
                workspace_base,
            )),
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
