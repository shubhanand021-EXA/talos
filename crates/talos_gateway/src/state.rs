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
    /// HEARTBEAT scheduler: owned schedules firing system-initiated tasks.
    pub scheduler: Arc<tokio::sync::Mutex<scheduler::Scheduler>>,
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
        Self::build_with_heartbeat(
            db,
            encryption_enabled,
            Some(layout.base.join("HEARTBEAT.md")),
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
        use policy_broker::{PolicyContext, PolicyDecision, PolicyEngine};
        let schedule = {
            let owned = self.scheduler.lock().await;
            owned.get(schedule_id).cloned()
        };
        let Some(schedule) = schedule else {
            return Err(TalosError::Storage(format!(
                "unknown schedule: {schedule_id}"
            )));
        };
        // Supervisor (DAG planner): deterministic keyword decomposition of the
        // system goal; recorded on the ledger so the plan is auditable.
        let supervisor = agent_runtime::supervisor::SupervisorAgent::new("supervisor-heartbeat");
        let subtasks = supervisor.decompose(&schedule.goal, None);
        let title = format!("[heartbeat] {}", schedule.name);
        let title = title.chars().take(256).collect::<String>();
        let mut seq = {
            let mut runtime = self.runtime.lock().await;
            runtime.next_queue_seq()
        };
        let view = agent_runtime::queue::submit_task(
            &self.db,
            &mut seq,
            agent_runtime::queue::SubmitParams {
                title,
                description: Some(schedule.goal.clone()),
                priority: "normal".to_owned(),
                timeout_ms: 30_000,
                max_attempts: 3,
                scenario: "quick_success".to_owned(),
                parent_task_id: None,
            },
        )
        .await?;
        let _ = evidence_store::append_event(
            &self.db,
            evidence_store::NewEvent {
                id: format!("scheduler-fired-{}-{}", schedule.id, view.id),
                kind: "scheduler.fired".to_owned(),
                payload_json: serde_json::json!({
                    "scheduleId": schedule.id,
                    "taskId": view.id,
                    "subtasks": subtasks.len(),
                    "via": "talos-gateway/1",
                }),
            },
        )
        .await;
        // Policy gate: the system action is evaluated like any agent action.
        let request = gate_request(&schedule, &view.id);
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
            // User may be away: walk queued -> planning -> waiting_policy
            // (direct queued -> waiting_policy is illegal) and notify the
            // desktop. The DAG decomposition above is the planning step.
            let mut current = agent_runtime::queue::get_task_state(&self.db, &view.id).await?;
            if agent_runtime::lifecycle::can_transition(&current.status, "planning") {
                current = agent_runtime::queue::transition_task(
                    &self.db,
                    &view.id,
                    "planning",
                    "heartbeat DAG planned",
                )
                .await?;
            }
            if agent_runtime::lifecycle::can_transition(&current.status, "waiting_policy") {
                let _ = agent_runtime::queue::transition_task(
                    &self.db,
                    &view.id,
                    "waiting_policy",
                    "heartbeat awaiting human review",
                )
                .await;
            }
            let _ = evidence_store::append_event(
                &self.db,
                evidence_store::NewEvent {
                    id: format!("scheduler-policy-{}-{}", schedule.id, view.id),
                    kind: "scheduler.policy_required".to_owned(),
                    payload_json: serde_json::json!({
                        "scheduleId": schedule.id,
                        "taskId": view.id,
                        "reason": evaluation.reason,
                    }),
                },
            )
            .await;
            let _ = self.events.send(crate::GatewayEvent::new(
                crate::GatewayEventKind::PolicyRequired,
                serde_json::json!({
                    "approvalId": format!("sched-approval-{}", view.id),
                    "taskId": view.id,
                    "summary": format!(
                        "Scheduled task '{}' needs human review: {}",
                        schedule.name, evaluation.reason
                    ),
                }),
                Some(view.id.clone()),
            ));
            let parked = agent_runtime::queue::get_task_state(&self.db, &view.id).await?;
            return Ok(parked);
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
        Ok(done)
    }
}

/// Build the policy gate request for a system-initiated schedule fire.
///
/// Deterministic heuristic on the goal text: goals naming mutating verbs
/// (`delete`, `exec`, `deploy`, ...) become High/non-reversible system-control
/// actions (which the broker escalates to human review); everything else is a
/// Low read-only action. The broker remains the sole authority — this only
/// shapes the request.
fn gate_request(
    schedule: &scheduler::RuntimeSchedule,
    task_id: &str,
) -> policy_broker::ActionRequest {
    use policy_broker::{Capability, RiskLevel};
    let goal_lower = schedule.goal.to_ascii_lowercase();
    let mutating = [
        "delete", "exec", "deploy", "publish", "install", "commit", "push", "write", "send",
        "remove", "format", "shutdown",
    ];
    let risky = mutating.iter().any(|verb| goal_lower.contains(verb));
    let (capability, risk_level, reversible) = if risky {
        (Capability::SystemControl, RiskLevel::High, false)
    } else {
        (Capability::ReadFile, RiskLevel::Low, true)
    };
    policy_broker::ActionRequest {
        id: format!("sched-gate-{}", &task_id[..task_id.len().min(64)]),
        agent_id: "scheduler".to_owned(),
        task_id: task_id.to_owned(),
        capability,
        intent: format!("heartbeat '{}': {}", schedule.name, schedule.goal)
            .chars()
            .take(1024)
            .collect(),
        target: "heartbeat".to_owned(),
        payload_json: serde_json::json!({ "scheduleId": schedule.id }),
        risk_level,
        reversible,
        requested_at: chrono::Utc::now(),
    }
}
