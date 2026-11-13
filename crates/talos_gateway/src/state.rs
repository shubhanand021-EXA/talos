//! Gateway runtime state: the single owner of the evidence database.
//!
//! The daemon holds the exclusive SQLite WAL lock through the existing v0.4.0
//! encryption envelope (`EvidenceDb::connect_encrypted_file` when a keychain
//! key is available, sealed-file fallback otherwise). `policy_broker`,
//! `agent_runtime` queue/supervisor, `context_assembler`, and the tool
//! registry all live here — never inside the Tauri window process.

use std::sync::Arc;

use talos_common::{Result, TalosError};

/// Default spend cap (USD) for unattended external tasks (scheduler fires,
/// channel messages). Conservative by design: matches the smallest tested
/// cap in-tree, so an owner-absent task can never run an open-ended bill.
/// Per-task overrides remain possible via `set_task_budget`; per-schedule
/// configuration is a follow-up (WP).
pub const DEFAULT_EXTERNAL_TASK_BUDGET_USD: f64 = 1.0;

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
    /// HEARTBEAT scheduler: owned schedules firing system-initiated tasks.
    pub scheduler: Arc<tokio::sync::Mutex<scheduler::Scheduler>>,
    /// OS-keychain vault (channel tokens resolve here, never touch disk).
    pub secrets: talos_secrets::SecretVault,
    /// Per-start WebSocket auth token (OS CSPRNG, regenerated every boot).
    /// Presented by clients as a `talos-gw-token.*` subprotocol element;
    /// never in URLs, never logged. Rotating on every start bounds the
    /// blast radius of a leaked token to one daemon lifetime.
    pub auth_token: String,
    /// External messaging channels (Telegram / Discord adapters).
    pub channels: Arc<tokio::sync::Mutex<channels::ChannelRegistry>>,
    /// Shared HTTP client for channel adapters.
    pub http: Arc<dyn llm_gateway::HttpClient>,
    /// App-data dir (channels.json persistence; `None` for in-memory tests).
    pub data_dir: Option<std::path::PathBuf>,
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
        Self::build_full(
            db,
            encryption_enabled,
            vault,
            Some(layout.base.join("HEARTBEAT.md")),
            Some(layout.base.clone()),
            Arc::new(
                llm_gateway::ReqwestHttpClient::new()
                    .map_err(|err| TalosError::Internal(format!("http client: {err}")))?,
            ),
        )
        .await
    }

    async fn build(db: evidence_store::EvidenceDb, encryption_enabled: bool) -> Result<Self> {
        Self::build_with_heartbeat(db, encryption_enabled, None).await
    }

    /// Build with an optional HEARTBEAT.md path (production passes
    /// `<app_data>/HEARTBEAT.md`; tests pass `None` for an empty scheduler).
    async fn build_with_heartbeat(
        db: evidence_store::EvidenceDb,
        encryption_enabled: bool,
        heartbeat_path: Option<std::path::PathBuf>,
    ) -> Result<Self> {
        let vault = talos_secrets::SecretVault::mock();
        let http: Arc<dyn llm_gateway::HttpClient> =
            Arc::new(llm_gateway::MockHttpClient::default());
        Self::build_full(db, encryption_enabled, vault, heartbeat_path, None, http).await
    }

    /// Full build: scheduler + channels + secrets + HTTP wiring.
    ///
    /// `data_dir` anchors `HEARTBEAT.md` (when `heartbeat_path` is `None`)
    /// and `.talos/channels.json` persistence; `None` keeps both empty.
    #[allow(clippy::too_many_arguments)]
    async fn build_full(
        db: evidence_store::EvidenceDb,
        encryption_enabled: bool,
        vault: talos_secrets::SecretVault,
        heartbeat_path: Option<std::path::PathBuf>,
        data_dir: Option<std::path::PathBuf>,
        http: Arc<dyn llm_gateway::HttpClient>,
    ) -> Result<Self> {
        let now = chrono::Utc::now();
        let owned = heartbeat_path
            .as_deref()
            .map(|path| scheduler::Scheduler::load_file(path, now))
            .unwrap_or_default();
        if !owned.is_empty() {
            eprintln!(
                "[talos-gateway] scheduler: {} schedule(s) loaded",
                owned.len()
            );
        }
        let (events, _) = tokio::sync::broadcast::channel(256);
        // Browser HITL fan-out: daemon-side pauses (rare; the desktop shell
        // usually owns the gate) broadcast `CaptchaDetected`/`AuthRequired`
        // so gateway-connected UIs render the intervention modal even when
        // the desktop window is closed. Process-local: each host installs
        // its own listener on the shared global registry.
        {
            let fanout = events.clone();
            browser_use::global_registry().set_listener(std::sync::Arc::new(move |event| {
                let kind = match event.kind {
                    browser_use::HitlKind::CaptchaDetected => {
                        crate::GatewayEventKind::CaptchaDetected
                    }
                    browser_use::HitlKind::AuthRequired => crate::GatewayEventKind::AuthRequired,
                };
                let _ = fanout.send(crate::GatewayEvent::new(
                    kind,
                    serde_json::json!({
                        "interventionId": event.intervention_id,
                        "taskId": event.task_id,
                        "kind": event.kind.as_str(),
                        "url": event.url,
                        "reason": event.reason,
                    }),
                    Some(event.task_id.clone()),
                ));
            }));
        }
        // Channels: secret-free config from <app_data>/channels.json (fail
        // closed when absent); adapters resolve tokens from the keychain.
        let channel_configs = data_dir
            .as_deref()
            .map(channels::ChannelConfigs::load_or_default)
            .unwrap_or_default();
        let registry =
            channels::ChannelRegistry::from_config(&channel_configs, &vault, http.clone());
        if !registry.is_empty() {
            eprintln!(
                "[talos-gateway] channels: {} adapter(s) linked",
                registry.len()
            );
        }
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
            scheduler: Arc::new(tokio::sync::Mutex::new(owned)),
            secrets: vault,
            auth_token: crate::protocol::generate_auth_token(),
            channels: Arc::new(tokio::sync::Mutex::new(registry)),
            http,
            data_dir,
            encryption_enabled,
            started_at: std::time::Instant::now(),
            events,
        })
    }

    /// Build around an in-memory database with a preloaded scheduler.
    ///
    /// Test and embedding hook: production uses [`GatewayState::new_persistent`].
    pub async fn new_in_memory_with_scheduler(schedules: scheduler::Scheduler) -> Result<Self> {
        let db = evidence_store::EvidenceDb::connect_memory().await?;
        let mut state = Self::build(db, false).await?;
        state.scheduler = Arc::new(tokio::sync::Mutex::new(schedules));
        Ok(state)
    }

    /// Build around an in-memory database with a preloaded channel registry.
    ///
    /// Test hook: injects [`channels::MockChannelAdapter`]s and a mock vault.
    pub async fn new_in_memory_with_channels(registry: channels::ChannelRegistry) -> Result<Self> {
        let db = evidence_store::EvidenceDb::connect_memory().await?;
        let mut state = Self::build(db, false).await?;
        state.channels = Arc::new(tokio::sync::Mutex::new(registry));
        Ok(state)
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

    /// Schedules owned by the daemon (read model for the UI Heartbeats panel).
    pub async fn list_schedules(&self) -> Vec<scheduler::ScheduleStatusView> {
        self.scheduler.lock().await.status()
    }

    /// Flip one schedule's active flag. `None` for an unknown id.
    pub async fn set_schedule_active(
        &self,
        id: &str,
        active: bool,
    ) -> Option<scheduler::ScheduleStatusView> {
        self.scheduler
            .lock()
            .await
            .set_active(id, active, chrono::Utc::now())
    }

    /// Fire one schedule immediately (manual override; paused schedules fire
    /// too). Records the outcome on the schedule.
    pub async fn trigger_schedule_now(&self, id: &str) -> Result<agent_runtime::queue::TaskView> {
        let view = self.submit_system_task(id).await;
        let outcome = match &view {
            Ok(view) => view.status.clone(),
            Err(_) => "failed".to_owned(),
        };
        self.scheduler
            .lock()
            .await
            .record_fire(id, &outcome, chrono::Utc::now());
        view
    }

    /// Run one scheduler tick: fire every due schedule as a system-initiated
    /// task. Never fails the daemon: per-schedule errors are recorded on the
    /// schedule and the tick continues.
    pub async fn run_scheduler_tick(&self) {
        let due: Vec<scheduler::RuntimeSchedule> =
            { self.scheduler.lock().await.tick_once(chrono::Utc::now()) };
        for schedule in due {
            let outcome = match self.submit_system_task(&schedule.id).await {
                Ok(view) => view.status.clone(),
                Err(err) => {
                    eprintln!("[talos-gateway] schedule {} failed: {err}", schedule.id);
                    "failed".to_owned()
                }
            };
            self.scheduler
                .lock()
                .await
                .record_fire(&schedule.id, &outcome, chrono::Utc::now());
        }
    }

    /// Fire one schedule as a system-initiated task through the Supervisor
    /// (DAG planner) and the PolicyBroker — no bypasses, ever.
    ///
    /// Flow: decompose the goal into a DAG (keyword path, no LLM needed) →
    /// submit the parent task → evaluate a policy gate for the system action →
    /// `RequireHumanReview` parks the task in `waiting_policy` and broadcasts
    /// `PolicyRequired` (desktop notification while the user is away) →
    /// otherwise execute through the broker and broadcast the lifecycle.
    pub async fn submit_system_task(
        &self,
        schedule_id: &str,
    ) -> Result<agent_runtime::queue::TaskView> {
        let schedule = {
            let owned = self.scheduler.lock().await;
            owned.get(schedule_id).cloned()
        };
        let Some(schedule) = schedule else {
            return Err(TalosError::Storage(format!(
                "unknown schedule: {schedule_id}"
            )));
        };
        let title = format!("[heartbeat] {}", schedule.name);
        let title = title.chars().take(256).collect::<String>();
        let name = schedule.name.clone();
        let (view, _outcome) = self
            .submit_external_task(
                &title,
                &schedule.goal,
                "scheduler",
                "scheduler",
                &|task_id, reason| {
                    (
                        format!("sched-approval-{task_id}"),
                        format!("Scheduled task '{name}' needs human review: {reason}"),
                    )
                },
            )
            .await?;
        Ok(view)
    }

    /// Shared external-task flow for scheduler fires and channel messages.
    ///
    /// The `notify` closure builds the `(approval_id, summary)` used when the
    /// broker escalates to human review. Returns the final task view plus the
    /// dispatch outcome the caller (scheduler recorder, channel replier) acts
    /// on. Every branch is ledgered; the broker is authoritative throughout.
    ///
    /// Unattended tasks are spend-capped by default
    /// ([`DEFAULT_EXTERNAL_TASK_BUDGET_USD`]): the cost governor denies any
    /// model/tool traffic past the cap, before it happens. The cap rides on
    /// the task row so per-task overrides remain possible later.
    async fn submit_external_task(
        &self,
        title: &str,
        goal: &str,
        agent_id: &str,
        kind_prefix: &str,
        notify: &(dyn Fn(&str, &str) -> (String, String) + Send + Sync),
    ) -> Result<(agent_runtime::queue::TaskView, ExternalOutcome)> {
        use policy_broker::{PolicyContext, PolicyDecision, PolicyEngine};
        // Supervisor (DAG planner): deterministic keyword decomposition of the
        // goal; recorded on the ledger so the plan is auditable.
        let supervisor = agent_runtime::supervisor::SupervisorAgent::new("supervisor-external");
        let subtasks = supervisor.decompose(goal, None);
        let mut seq = {
            let mut runtime = self.runtime.lock().await;
            runtime.next_queue_seq()
        };
        let view = agent_runtime::queue::submit_task(
            &self.db,
            &mut seq,
            agent_runtime::queue::SubmitParams {
                title: title.to_owned(),
                description: Some(goal.to_owned()),
                priority: "normal".to_owned(),
                timeout_ms: 30_000,
                max_attempts: 3,
                scenario: "quick_success".to_owned(),
                parent_task_id: None,
            },
        )
        .await?;
        // Unattended default cap: enforced by the cost governor before any
        // model/tool traffic (llm_gateway budget gate reads this row).
        // Fail-closed: a budget write failure refuses the task rather than
        // running it uncapped.
        evidence_store::set_task_budget(&self.db, &view.id, Some(DEFAULT_EXTERNAL_TASK_BUDGET_USD))
            .await?;
        let _ = evidence_store::append_event(
            &self.db,
            evidence_store::NewEvent {
                id: format!("{kind_prefix}-fired-{}", view.id),
                kind: format!("{kind_prefix}.fired"),
                payload_json: serde_json::json!({
                    "taskId": view.id,
                    "agent": agent_id,
                    "subtasks": subtasks.len(),
                    "via": "talos-gateway/1",
                }),
            },
        )
        .await;
        // Policy gate: the external action is evaluated like any agent action.
        let request = build_gate_request(goal, title, &view.id, agent_id);
        let has_evidence = evidence_store::has_evidence_for_task(&self.db, &view.id)
            .await
            .unwrap_or(false);
        let has_plan = evidence_store::has_compensation_plan(&self.db, &request.id)
            .await
            .unwrap_or(false);
        let evaluation = self.engine.evaluate_with_context(
            &request,
            &PolicyContext {
                has_evidence,
                has_compensation_plan: has_plan,
            },
        );
        if evaluation.decision == PolicyDecision::RequireHumanReview {
            // Requester may be away: walk queued -> planning -> waiting_policy
            // (direct queued -> waiting_policy is illegal) and notify.
            let mut current = agent_runtime::queue::get_task_state(&self.db, &view.id).await?;
            if agent_runtime::lifecycle::can_transition(&current.status, "planning") {
                current = agent_runtime::queue::transition_task(
                    &self.db,
                    &view.id,
                    "planning",
                    "external DAG planned",
                )
                .await?;
            }
            if agent_runtime::lifecycle::can_transition(&current.status, "waiting_policy") {
                let _ = agent_runtime::queue::transition_task(
                    &self.db,
                    &view.id,
                    "waiting_policy",
                    "external task awaiting human review",
                )
                .await;
            }
            let (approval_id, summary) = notify(&view.id, &evaluation.reason);
            let _ = evidence_store::append_event(
                &self.db,
                evidence_store::NewEvent {
                    id: format!("{kind_prefix}-policy-{}", view.id),
                    kind: format!("{kind_prefix}.policy_required"),
                    payload_json: serde_json::json!({
                        "taskId": view.id,
                        "reason": evaluation.reason,
                    }),
                },
            )
            .await;
            let _ = self.events.send(crate::GatewayEvent::new(
                crate::GatewayEventKind::PolicyRequired,
                serde_json::json!({
                    "approvalId": approval_id,
                    "taskId": view.id,
                    "summary": summary,
                }),
                Some(view.id.clone()),
            ));
            let parked = agent_runtime::queue::get_task_state(&self.db, &view.id).await?;
            return Ok((
                parked.clone(),
                ExternalOutcome::WaitingPolicy(evaluation.reason),
            ));
        }
        if evaluation.decision != PolicyDecision::Allow {
            // Refused (Deny / RequireEvidence / RequireCompensation): the task
            // must never execute. Walk to `failed` with the broker's reason.
            let mut current = agent_runtime::queue::get_task_state(&self.db, &view.id).await?;
            if agent_runtime::lifecycle::can_transition(&current.status, "planning") {
                current = agent_runtime::queue::transition_task(
                    &self.db,
                    &view.id,
                    "planning",
                    "external DAG planned",
                )
                .await?;
            }
            if agent_runtime::lifecycle::can_transition(&current.status, "failed") {
                let reason: String = evaluation.reason.chars().take(256).collect();
                let _ =
                    agent_runtime::queue::transition_task(&self.db, &view.id, "failed", &reason)
                        .await;
            }
            let _ = evidence_store::append_event(
                &self.db,
                evidence_store::NewEvent {
                    id: format!("{kind_prefix}-denied-{}", view.id),
                    kind: format!("{kind_prefix}.denied"),
                    payload_json: serde_json::json!({
                        "taskId": view.id,
                        "decision": format!("{:?}", evaluation.decision),
                        "reason": evaluation.reason,
                    }),
                },
            )
            .await;
            let failed = agent_runtime::queue::get_task_state(&self.db, &view.id).await?;
            let _ = self.events.send(crate::GatewayEvent::new(
                crate::GatewayEventKind::TaskCompleted,
                serde_json::json!({ "taskId": failed.id, "status": failed.status.clone() }),
                Some(failed.id.clone()),
            ));
            return Ok((failed.clone(), ExternalOutcome::Refused(evaluation.reason)));
        }
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
        Ok((done.clone(), ExternalOutcome::Allowed(Box::new(done))))
    }

    /// Handle one inbound channel message: allowlist → parse → Supervisor +
    /// PolicyBroker → reply. Never fails the poll loop: per-message errors
    /// are ledgered and answered with a refusal.
    pub async fn handle_channel_message(
        &self,
        channel: channels::ChannelId,
        message: &channels::IncomingMessage,
    ) {
        let allowed = {
            let registry = self.channels.lock().await;
            let configs = registry.configs();
            match channel {
                channels::ChannelId::Telegram => {
                    configs.telegram.enabled
                        && configs
                            .telegram
                            .allowed_chat_ids
                            .iter()
                            .any(|id| id == &message.chat_id)
                }
                channels::ChannelId::Discord => {
                    configs.discord.enabled
                        && configs
                            .discord
                            .allowed_channel_ids
                            .iter()
                            .any(|id| id == &message.chat_id)
                }
            }
        };
        if !allowed {
            let _ = evidence_store::append_event(
                &self.db,
                evidence_store::NewEvent {
                    id: format!(
                        "channel-rejected-{}-{}",
                        channel.as_str(),
                        message.message_id
                    ),
                    kind: "channel.rejected".to_owned(),
                    payload_json: serde_json::json!({
                        "channel": channel.as_str(),
                        "messageId": message.message_id,
                        "reason": "sender not allowlisted",
                    }),
                },
            )
            .await;
            return;
        }
        let task = match message.parse_task() {
            Ok(task) => task,
            Err(err) => {
                self.reply(
                    channel,
                    &message.chat_id,
                    &channels::render_reply("denied", "message not understood", &err.to_string()),
                )
                .await;
                return;
            }
        };
        let _ = evidence_store::append_event(
            &self.db,
            evidence_store::NewEvent {
                id: format!(
                    "channel-received-{}-{}",
                    channel.as_str(),
                    message.message_id
                ),
                kind: "channel.received".to_owned(),
                payload_json: serde_json::json!({
                    "channel": channel.as_str(),
                    "messageId": message.message_id,
                    "title": task.title,
                }),
            },
        )
        .await;
        let agent_id = format!("channel-{}", channel.as_str());
        let title = format!("[{}] {}", channel.as_str(), task.title);
        let title = title.chars().take(256).collect::<String>();
        let channel_name = channel.as_str().to_owned();
        let (view, outcome) = match self
            .submit_external_task(
                &title,
                &task.goal,
                &agent_id,
                "channel",
                &|task_id, reason| {
                    (
                        format!("channel-approval-{task_id}"),
                        format!("Task from {channel_name} needs human review: {reason}"),
                    )
                },
            )
            .await
        {
            Ok(result) => result,
            Err(err) => {
                self.reply(
                    channel,
                    &message.chat_id,
                    &channels::render_reply("denied", &task.title, &err.to_string()),
                )
                .await;
                return;
            }
        };
        let reply = match outcome {
            ExternalOutcome::Allowed(done) => channels::render_reply(
                "completed",
                &task.title,
                &format!("task {} · status {}", done.id, done.status),
            ),
            ExternalOutcome::WaitingPolicy(reason) => channels::render_reply(
                "waiting_policy",
                &task.title,
                &format!("task {} parked for desktop approval: {}", view.id, reason),
            ),
            ExternalOutcome::Refused(reason) => {
                channels::render_reply("denied", &task.title, &reason)
            }
        };
        self.reply(channel, &message.chat_id, &reply).await;
    }

    /// Best-effort reply delivery (transport failures only log).
    async fn reply(&self, channel: channels::ChannelId, to: &str, text: &str) {
        if let Err(err) = self
            .channels
            .lock()
            .await
            .send_reply(channel, to, text)
            .await
        {
            eprintln!("[talos-gateway] channel reply failed: {err}");
        }
    }

    /// Poll every linked channel once and route inbound messages. Never fails
    /// the daemon: per-message errors are contained in
    /// [`GatewayState::handle_channel_message`].
    pub async fn poll_channels(&self) {
        let inbound = { self.channels.lock().await.poll_once().await };
        for (channel, message) in inbound {
            self.handle_channel_message(channel, &message).await;
        }
    }

    /// Channel status rows for the UI Channels tab (never carries tokens).
    pub async fn list_channels(&self) -> Vec<channels::ChannelStatusView> {
        self.channels.lock().await.status(&self.secrets)
    }

    /// Recent channel traffic for the UI log (metadata + excerpts).
    pub async fn list_channel_messages(&self, limit: usize) -> Vec<channels::LoggedMessage> {
        self.channels.lock().await.recent_messages(limit)
    }

    /// Link a channel: store the token in the keychain, enable, attach a
    /// live adapter, persist the secret-free config.
    pub async fn link_channel(
        &self,
        channel: channels::ChannelId,
        token: &str,
    ) -> Result<Vec<channels::ChannelStatusView>> {
        {
            let mut registry = self.channels.lock().await;
            registry.link_channel(channel, token, &self.secrets)?;
            let adapter: std::sync::Arc<dyn channels::ChannelAdapter> = match channel {
                channels::ChannelId::Telegram => std::sync::Arc::new(
                    channels::TelegramAdapter::new(token.to_owned(), self.http.clone()),
                ),
                channels::ChannelId::Discord => {
                    let webhook = if token.trim_start().starts_with("https://") {
                        Some(token.to_owned())
                    } else {
                        None
                    };
                    let bot_token = if webhook.is_some() {
                        None
                    } else {
                        Some(token.to_owned())
                    };
                    let channel_id = registry.configs().discord.channel_id.clone();
                    std::sync::Arc::new(channels::DiscordAdapter::new(
                        bot_token,
                        webhook,
                        channel_id,
                        self.http.clone(),
                    ))
                }
            };
            registry.add_adapter(adapter);
            if let Some(dir) = self.data_dir.as_deref() {
                if let Err(err) = registry.configs().save(dir) {
                    eprintln!("[talos-gateway] channels.json save failed: {err}");
                }
            }
        }
        Ok(self.list_channels().await)
    }

    /// Unlink a channel: drop credentials, disable, detach the adapter.
    pub async fn unlink_channel(
        &self,
        channel: channels::ChannelId,
    ) -> Result<Vec<channels::ChannelStatusView>> {
        {
            let mut registry = self.channels.lock().await;
            registry.unlink_channel(channel, &self.secrets)?;
            if let Some(dir) = self.data_dir.as_deref() {
                if let Err(err) = registry.configs().save(dir) {
                    eprintln!("[talos-gateway] channels.json save failed: {err}");
                }
            }
        }
        Ok(self.list_channels().await)
    }
}

/// Dispatch verdict for one external task (scheduler fire or channel message).
#[derive(Debug, Clone)]
enum ExternalOutcome {
    /// Broker allowed and the worker executed.
    Allowed(Box<agent_runtime::queue::TaskView>),
    /// Broker escalated; parked in `waiting_policy` with the reason.
    WaitingPolicy(String),
    /// Broker refused; task terminal `failed` with the reason.
    Refused(String),
}

/// Build the policy gate request for an external goal (schedule fire or
/// channel message).
///
/// Deterministic shape via [`channels::classify_goal`]: reads → Low allow;
/// scoped mutations → Medium evidence-gated (refused from external surfaces
/// that cannot produce evidence); high-risk → High human review; wildcard
/// destructive → least-privilege shape the broker's default-deny rejects.
/// The broker remains the sole authority — this only shapes the request.
fn build_gate_request(
    goal: &str,
    label: &str,
    task_id: &str,
    agent_id: &str,
) -> policy_broker::ActionRequest {
    let shape = channels::classify_goal(goal);
    policy_broker::ActionRequest {
        id: format!("ext-gate-{}", &task_id[..task_id.len().min(64)]),
        agent_id: agent_id.to_owned(),
        task_id: task_id.to_owned(),
        capability: shape.capability,
        intent: format!("{label}: {goal}").chars().take(1024).collect(),
        target: "external".to_owned(),
        payload_json: serde_json::json!({ "agent": agent_id }),
        risk_level: shape.risk,
        reversible: shape.reversible,
        requested_at: chrono::Utc::now(),
    }
}
