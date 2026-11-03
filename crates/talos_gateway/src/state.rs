//! Gateway runtime state: the single owner of the evidence database.
//!
//! The daemon holds the exclusive SQLite WAL lock through the existing v0.4.0
//! encryption envelope (`EvidenceDb::connect_encrypted_file` when a keychain
//! key is available, sealed-file fallback otherwise). `policy_broker`,
//! `agent_runtime` queue/supervisor, `context_assembler`, and the tool
//! registry all live here — never inside the Tauri window process.

use std::sync::Arc;

use talos_common::{Result, TalosError};

/// Shared daemon state (cheap to clone: pools + registries behind handles).
#[derive(Debug, Clone)]
pub struct GatewayState {
    /// Exclusive evidence database handle (WAL + envelope encryption).
    pub db: evidence_store::EvidenceDb,
    /// Policy authority for every side-effecting action.
    pub engine: Arc<policy_broker::DefaultPolicyEngine>,
    /// Tool registry (enablement flags; manifests are static).
    pub tools: Arc<tokio::sync::Mutex<tool_runtime::ToolRegistry>>,
    /// Local orchestration runtime (durable queue worker + supervisor).
    pub runtime: Arc<tokio::sync::Mutex<agent_runtime::LocalOrchestrator>>,
    /// Context assembly budget configuration (validated percentages).
    pub context_config: Arc<tokio::sync::Mutex<context_assembler::ContextAssemblyConfig>>,
    /// Supervisor tuning (fan-out/depth enforced per spawn).
    pub supervisor_config: Arc<tokio::sync::Mutex<agent_runtime::supervisor::SupervisorConfig>>,
    /// Whether the evidence database is encrypted at rest.
    pub encryption_enabled: bool,
    /// Process launch time (uptime reporting).
    pub started_at: std::time::Instant,
    /// Event broadcast channel (daemon → every connected client).
    pub events: tokio::sync::broadcast::Sender<crate::GatewayEvent>,
}

/// Public status snapshot for `GetStatus` / heartbeat consumers.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayStatus {
    /// Protocol tag (always `talos-gateway/1`).
    pub protocol: String,
    /// Daemon crate version.
    pub version: String,
    /// Seconds since daemon start.
    pub uptime_seconds: f64,
    /// Queue depth (durable tasks tracked).
    pub active_tasks: u32,
    /// Policy broker liveness (always true when serving).
    pub policy_broker_healthy: bool,
    /// Ledger readability probe.
    pub evidence_ledger_healthy: bool,
    /// At-rest encryption state.
    pub encryption_enabled: bool,
}

impl GatewayState {
    /// Build state around an isolated in-memory database (tests only).
    pub async fn new_in_memory() -> Result<Self> {
        let db = evidence_store::EvidenceDb::connect_memory().await?;
        Self::build(db, false).await
    }

    /// Build state around the durable file database under `app_data_dir`.
    ///
    /// Production entry point: resolves `<app_data_dir>/talos.db`, unlocks or
    /// creates the OS-keychain database key, opens encrypted at rest
    /// (plaintext fallback only when the keychain is unreachable), runs
    /// startup crash recovery, and logs the resolved path (path only).
    pub async fn new_persistent(app_data_dir: &std::path::Path) -> Result<Self> {
        Self::new_persistent_with_vault(app_data_dir, talos_secrets::SecretVault::os()).await
    }

    /// Build durable state with an explicit secret vault.
    pub async fn new_persistent_with_vault(
        app_data_dir: &std::path::Path,
        vault: talos_secrets::SecretVault,
    ) -> Result<Self> {
        let layout = evidence_store::storage::ensure_layout(app_data_dir)?;
        eprintln!("[talos-gateway] data directory: {}", layout.base.display());
        let (db, encryption_enabled) = match evidence_store::load_or_create_key(&vault) {
            Ok(evidence_store::KeyState::Encrypted(key)) => {
                let db = evidence_store::EvidenceDb::connect_encrypted_file(&layout.db_file, &key)
                    .await?;
                eprintln!("[talos-gateway] database: encrypted at rest");
                (db, true)
            }
            Ok(evidence_store::KeyState::Unavailable(reason)) => {
                eprintln!(
                    "[talos-gateway] WARNING: keychain unavailable ({reason}); RUNNING UNENCRYPTED"
                );
                let db = evidence_store::EvidenceDb::connect_file(&layout.db_file).await?;
                (db, false)
            }
            Err(err) => return Err(err),
        };
        eprintln!(
            "[talos-gateway] database: {} (journal_mode={})",
            layout.db_file.display(),
            db.journal_mode()
                .await
                .unwrap_or_else(|_| "unknown".to_owned())
        );
        let recovery = evidence_store::run_startup_recovery(&db).await?;
        eprintln!(
            "[talos-gateway] startup recovery: chain_valid={} recovered={} dirty_shutdown={}",
            recovery.chain_valid,
            recovery.recovered.len(),
            recovery.dirty_shutdown,
        );
        Self::build(db, encryption_enabled).await
    }

    async fn build(db: evidence_store::EvidenceDb, encryption_enabled: bool) -> Result<Self> {
        let (events, _) = tokio::sync::broadcast::channel(256);
        // Background supervisor tick: keeps the durable queue warm between
        // client connections (the daemon lives even with zero sockets open).
        Ok(Self {
            db,
            engine: Arc::new(policy_broker::DefaultPolicyEngine::new()),
            tools: Arc::new(tokio::sync::Mutex::new(
                tool_runtime::ToolRegistry::with_builtins(),
            )),
            runtime: Arc::new(tokio::sync::Mutex::new(
                agent_runtime::LocalOrchestrator::new(),
            )),
            context_config: Arc::new(tokio::sync::Mutex::new(
                context_assembler::ContextAssemblyConfig::default(),
            )),
            supervisor_config: Arc::new(tokio::sync::Mutex::new(
                agent_runtime::supervisor::SupervisorConfig::default(),
            )),
            encryption_enabled,
            started_at: std::time::Instant::now(),
            events,
        })
    }

    /// Current status snapshot (serves `GetStatus` + heartbeat payloads).
    pub async fn status(&self) -> GatewayStatus {
        let active_tasks = evidence_store::list_tasks(&self.db, 1000)
            .await
            .map(|rows| rows.len())
            .unwrap_or(0);
        let evidence_ledger_healthy = evidence_store::get_integrity_report(&self.db)
            .await
            .map(|report| report.healthy)
            .unwrap_or(false);
        GatewayStatus {
            protocol: crate::GATEWAY_PROTOCOL_VERSION.to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            uptime_seconds: self.started_at.elapsed().as_secs_f64(),
            active_tasks: u32::try_from(active_tasks).unwrap_or(u32::MAX),
            policy_broker_healthy: true,
            evidence_ledger_healthy,
            encryption_enabled: self.encryption_enabled,
        }
    }

    /// Enqueue a durable task, execute it through the policy broker, and
    /// broadcast `TaskStarted` → `TaskCompleted` on the event channel.
    pub async fn submit_task(
        &self,
        payload: crate::GatewaySubmitTask,
    ) -> Result<agent_runtime::queue::TaskView> {
        let title = payload.title.trim().to_owned();
        if title.is_empty() || title.len() > 256 {
            return Err(TalosError::Storage(
                "title must be 1..=256 chars".to_owned(),
            ));
        }
        let mut seq = {
            let mut runtime = self.runtime.lock().await;
            runtime.next_queue_seq()
        };
        let view = agent_runtime::queue::submit_task(
            &self.db,
            &mut seq,
            agent_runtime::queue::SubmitParams {
                title: title.clone(),
                description: payload.description.clone(),
                priority: payload.priority.clone(),
                timeout_ms: 30_000,
                max_attempts: 3,
                scenario: payload.scenario.clone(),
                parent_task_id: None,
            },
        )
        .await?;
        let _ = self.events.send(crate::GatewayEvent::new(
            crate::GatewayEventKind::TaskStarted,
            serde_json::json!({ "taskId": view.id, "title": view.title }),
            Some(view.id.clone()),
        ));
        let done = agent_runtime::queue::execute_task(
            &self.db,
            self.engine.as_ref(),
            &view.id,
            &agent_runtime::queue::CancelToken::new(),
        )
        .await?;
        let _ = self.events.send(crate::GatewayEvent::new(
            crate::GatewayEventKind::TaskCompleted,
            serde_json::json!({ "taskId": done.id, "status": done.status }),
            Some(done.id.clone()),
        ));
        Ok(done)
    }

    /// Request cancellation for one durable task.
    pub async fn cancel_task(&self, task_id: &str) -> Result<agent_runtime::queue::TaskView> {
        agent_runtime::queue::cancel_task(&self.db, task_id).await
    }
}
