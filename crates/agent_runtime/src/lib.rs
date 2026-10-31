//! Local agent runtime: deterministic orchestration plus a durable
//! task queue with lifecycle supervision.
//!
//! [`LocalOrchestrator`] keeps the Batch 1 diagnostic loop (no network, no
//! LLMs). New modules add production-oriented foundations without breaking
//! that loop: [`lifecycle`] validates every status move, [`queue`] persists
//! queue entries/attempts/transitions/cancellations in SQLite and executes
//! tasks through the policy broker, and [`supervisor`] enforces timeouts,
//! cancellation, and policy-respecting retries. [`llm_agent`] adds the
//! model-backed agent: it routes through the LLM gateway when a provider key
//! is configured and falls back to the deterministic mock path otherwise.

use policy_broker::{
    ActionRequest, Capability, PolicyDecision, PolicyEngine, PolicyEvaluation, RiskLevel,
};
use talos_common::{now_iso, Result};

pub mod lifecycle;
pub mod llm_agent;
pub mod queue;
pub mod supervisor;

/// Lifecycle of a runtime-managed task.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RuntimeTask {
    /// Task identifier (`diag-task-{run}`).
    pub id: String,
    /// Human-readable title.
    pub title: String,
    /// `queued` | `executing` | `completed`.
    pub status: String,
}

/// Lifecycle of a runtime-managed agent.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RuntimeAgent {
    /// Agent identifier (`diag-agent-{run}`).
    pub id: String,
    /// Owning task identifier.
    pub task_id: String,
    /// Display name.
    pub display_name: String,
    /// Agent role (mirrors the contracts `AgentRole` union).
    pub role: String,
    /// `idle` | `running` | `completed`.
    pub status: String,
}

/// One observable stage of a diagnostic run. The Tauri layer maps each
/// variant onto its contracts event name plus a JSON payload.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum RuntimeEvent {
    /// A task was created.
    TaskCreated {
        /// Task identifier.
        task_id: String,
        /// Task title.
        title: String,
    },
    /// An agent spawned for a task.
    AgentSpawned {
        /// Agent identifier.
        agent_id: String,
        /// Owning task identifier.
        task_id: String,
        /// Display name.
        display_name: String,
        /// Agent role.
        role: String,
    },
    /// The echo agent proposed an action.
    ActionProposed {
        /// Proposed request identifier.
        request_id: String,
    },
    /// The policy engine returned a verdict.
    PolicyEvaluated {
        /// Evaluated request identifier.
        request_id: String,
        /// Verdict.
        decision: PolicyDecision,
    },
    /// Evidence landed in the ledger.
    EvidenceRecorded {
        /// Evidence identifier.
        evidence_id: String,
    },
    /// Evidence transitioned to `verified`.
    EvidenceVerified {
        /// Evidence identifier.
        evidence_id: String,
    },
    /// A claim was raised for the run.
    ClaimCreated {
        /// Claim identifier.
        claim_id: String,
    },
    /// A compensation plan was recorded.
    CompensationPlanned {
        /// Plan identifier.
        plan_id: String,
    },
    /// A claim reached its terminal evaluated status.
    ClaimEvaluated {
        /// Claim identifier.
        claim_id: String,
        /// Resulting status.
        status: String,
    },
    /// The task reached a terminal state.
    TaskCompleted {
        /// Task identifier.
        task_id: String,
        /// One-line outcome.
        summary: String,
    },
    /// A durable queue task changed lifecycle status.
    TaskStatusChanged {
        /// Task identifier.
        task_id: String,
        /// Previous status.
        from: String,
        /// New status.
        to: String,
    },
}

/// Full result of one diagnostic run (returned to the IPC layer).
#[derive(Debug, Clone)]
pub struct DiagnosticRun {
    /// Task created for this run.
    pub task: RuntimeTask,
    /// Agent spawned for this run.
    pub agent: RuntimeAgent,
    /// First action the echo agent proposed (low-risk read).
    pub request: ActionRequest,
    /// Verdict for the first proposal.
    pub evaluation: PolicyEvaluation,
    /// Evidence row recorded for the verdict.
    pub evidence_id: String,
    /// All policy verdicts in stage order (read, write, irreversible).
    pub evaluations: Vec<PolicyEvaluation>,
    /// Evidence rows recorded, in stage order.
    pub evidence_ids: Vec<String>,
    /// Claim raised for the completed run.
    pub claim_id: String,
    /// Compensation plan created for the irreversible stage.
    pub plan_id: String,
    /// Ordered lifecycle events (mapped to Tauri events by the caller).
    pub events: Vec<RuntimeEvent>,
}

/// Mutable orchestration state: tasks, agents, and a run counter that keeps
/// identifiers deterministic (`diag-{kind}-{run}`).
#[derive(Debug, Default)]
pub struct RuntimeState {
    /// Tasks by id, insertion ordered.
    pub tasks: Vec<RuntimeTask>,
    /// Agents by id, insertion ordered.
    pub agents: Vec<RuntimeAgent>,
    /// Completed diagnostic runs (monotonic; drives deterministic ids).
    pub run_count: u64,
}

/// Deterministic echo agent: no LLM, no network. Proposes the same three
/// diagnostic actions every run, so policy verdicts are stable.
#[derive(Debug, Default)]
pub struct MockEchoAgent {
    /// Seeded configuration for durable-queue scenarios (default: quick).
    pub config: EchoConfig,
}

/// Deterministic mock scenario (seeded; no randomness, no network).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EchoScenario {
    /// Immediate policy-allowed success.
    #[default]
    QuickSuccess,
    /// Short deterministic delay, then success.
    DelayedSuccess,
    /// First attempt fails retryably, later attempts succeed.
    RetryableFailure,
    /// Always fails non-retryably.
    NonRetryableFailure,
    /// Checks cancellation between deterministic steps.
    CancellationSensitive,
}

/// Seeded configuration for [`MockEchoAgent`] scenarios.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EchoConfig {
    /// Active scenario.
    pub scenario: EchoScenario,
    /// Deterministic seed (drives ids/delays, never `rand`).
    pub seed: u64,
    /// Delay for [`EchoScenario::DelayedSuccess`] in milliseconds.
    pub delay_ms: u64,
}

impl Default for EchoConfig {
    fn default() -> Self {
        Self {
            scenario: EchoScenario::QuickSuccess,
            seed: 1,
            delay_ms: 120,
        }
    }
}

impl MockEchoAgent {
    /// Create the echo agent.
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: EchoConfig::default(),
        }
    }

    /// Create the echo agent with a seeded scenario configuration.
    #[must_use]
    pub fn with_config(config: EchoConfig) -> Self {
        Self { config }
    }

    /// Deterministic outcome for one queue attempt (no I/O).
    ///
    /// Returns `(succeeded, retryable)`: `RetryableFailure` fails retryably
    /// on attempt 1 and succeeds after; `NonRetryableFailure` always fails
    /// fatally; all others succeed. The seed only scopes identifiers.
    #[must_use]
    pub fn outcome_for_attempt(&self, attempt: i64) -> (bool, bool) {
        match self.config.scenario {
            EchoScenario::QuickSuccess | EchoScenario::DelayedSuccess => (true, false),
            EchoScenario::CancellationSensitive => (true, false),
            EchoScenario::RetryableFailure => {
                if attempt <= 1 {
                    (false, true)
                } else {
                    (true, false)
                }
            }
            EchoScenario::NonRetryableFailure => (false, false),
        }
    }

    /// Propose the canonical low-risk diagnostic read for `task_id`.
    #[must_use]
    pub fn plan_action(&self, agent_id: &str, task_id: &str, run: u64) -> ActionRequest {
        ActionRequest {
            id: format!("diag-req-{run}"),
            agent_id: agent_id.to_owned(),
            task_id: task_id.to_owned(),
            capability: Capability::ReadFile,
            intent: "Echo diagnostic read of the local run manifest".to_owned(),
            target: format!("/tmp/talos-diag-{run}.txt"),
            payload_json: serde_json::json!({ "run": run }),
            risk_level: RiskLevel::Low,
            reversible: true,
            requested_at: chrono::Utc::now(),
        }
    }

    /// Propose a write-like action that must gate behind recorded evidence.
    #[must_use]
    pub fn plan_write_action(&self, agent_id: &str, task_id: &str, run: u64) -> ActionRequest {
        ActionRequest {
            id: format!("diag-write-{run}"),
            agent_id: agent_id.to_owned(),
            task_id: task_id.to_owned(),
            capability: Capability::WriteFile,
            intent: "Echo diagnostic write of the local run checkpoint".to_owned(),
            target: format!("/tmp/talos-checkpoint-{run}.txt"),
            payload_json: serde_json::json!({ "run": run }),
            risk_level: RiskLevel::Medium,
            reversible: true,
            requested_at: chrono::Utc::now(),
        }
    }

    /// Propose an irreversible action that must gate behind a compensation plan.
    #[must_use]
    pub fn plan_irreversible_action(
        &self,
        agent_id: &str,
        task_id: &str,
        run: u64,
    ) -> ActionRequest {
        ActionRequest {
            id: format!("diag-rotate-{run}"),
            agent_id: agent_id.to_owned(),
            task_id: task_id.to_owned(),
            capability: Capability::SystemControl,
            intent: "Echo diagnostic rotation of the local run key".to_owned(),
            target: format!("keyring:talos-diag-{run}"),
            payload_json: serde_json::json!({ "run": run }),
            risk_level: RiskLevel::Irreversible,
            reversible: false,
            requested_at: chrono::Utc::now(),
        }
    }

    /// Summarize the outcome of an evaluated proposal.
    #[must_use]
    pub fn execute_summary(&self, evaluation: &PolicyEvaluation) -> String {
        format!(
            "echo action {} with {:?}",
            evaluation.request_id, evaluation.decision
        )
    }
}

/// In-memory orchestrator for the local control plane.
#[derive(Debug, Default)]
pub struct LocalOrchestrator {
    state: RuntimeState,
    echo: MockEchoAgent,
    queue_counter: u64,
    queue_config: queue::RuntimeConfig,
}

impl LocalOrchestrator {
    /// Create an empty orchestrator.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: RuntimeState::default(),
            echo: MockEchoAgent::new(),
            queue_counter: 0,
            queue_config: queue::RuntimeConfig::default(),
        }
    }

    /// Borrow the current runtime state (tasks, agents, run count).
    #[must_use]
    pub fn state(&self) -> &RuntimeState {
        &self.state
    }

    /// Borrow the durable-queue configuration.
    #[must_use]
    pub fn queue_config(&self) -> &queue::RuntimeConfig {
        &self.queue_config
    }

    /// Next durable queue sequence number (monotonic, deterministic).
    pub fn next_queue_seq(&mut self) -> u64 {
        self.queue_counter += 1;
        self.queue_counter
    }

    /// Run one deterministic diagnostic loop against `engine` + `db`.
    ///
    /// Identifiers embed the 1-based run number, so repeated runs are
    /// stable apart from timestamps and the counter itself. The loop walks
    /// three gated stages — low-risk read (allow), write-like (evidence),
    /// irreversible (compensation plan) — then raises a verified claim.
    pub async fn run_diagnostic(
        &mut self,
        engine: &dyn PolicyEngine,
        db: &evidence_store::EvidenceDb,
    ) -> Result<DiagnosticRun> {
        use evidence_store as store;

        self.state.run_count += 1;
        let run = self.state.run_count;
        let mut events = Vec::new();
        let mut evaluations = Vec::new();
        let mut evidence_ids = Vec::new();

        // Stage 1 — create the task.
        let mut task = RuntimeTask {
            id: format!("diag-task-{run}"),
            title: format!("Local diagnostic run {run}"),
            status: "executing".to_owned(),
        };
        self.state.tasks.push(task.clone());
        events.push(RuntimeEvent::TaskCreated {
            task_id: task.id.clone(),
            title: task.title.clone(),
        });

        // Stage 2 — spawn the echo agent.
        let mut agent = RuntimeAgent {
            id: format!("diag-agent-{run}"),
            task_id: task.id.clone(),
            display_name: format!("Echo Unit {run}"),
            role: "researcher".to_owned(),
            status: "running".to_owned(),
        };
        self.state.agents.push(agent.clone());
        events.push(RuntimeEvent::AgentSpawned {
            agent_id: agent.id.clone(),
            task_id: agent.task_id.clone(),
            display_name: agent.display_name.clone(),
            role: agent.role.clone(),
        });

        // Helper: record request + evaluation in the audit trail, then
        // evaluate with the given ledger context.
        async fn evaluate_stage(
            engine: &dyn PolicyEngine,
            db: &evidence_store::EvidenceDb,
            request: &ActionRequest,
            ctx: &policy_broker::PolicyContext,
            policy_version: &str,
        ) -> Result<PolicyEvaluation> {
            let evaluation = engine.evaluate_with_context(request, ctx);
            store::record_action_request(
                db,
                &evidence_store::NewActionRecord {
                    id: request.id.clone(),
                    agent_id: request.agent_id.clone(),
                    task_id: request.task_id.clone(),
                    capability: format!("{:?}", request.capability),
                    intent: request.intent.clone(),
                    target: request.target.clone(),
                    payload: request.payload_json.clone(),
                    risk_level: format!("{:?}", request.risk_level),
                    reversible: request.reversible,
                    requested_at: request.requested_at.to_rfc3339(),
                },
                policy_version,
                &format!("{:?}", evaluation.decision),
                &evaluation.reason,
                &evaluation.required_checks,
            )
            .await?;
            Ok(evaluation)
        }

        // Helper: record evidence for a verdict and mark it verified.
        struct VerifiedEvidence<'a> {
            id: String,
            task_id: &'a str,
            agent_id: &'a str,
            label: String,
            summary: String,
            decision: PolicyDecision,
            request_id: &'a str,
        }
        async fn record_verified_evidence(
            db: &evidence_store::EvidenceDb,
            spec: VerifiedEvidence<'_>,
        ) -> Result<String> {
            store::insert_evidence(
                db,
                store::NewEvidence {
                    id: spec.id.clone(),
                    task_id: Some(spec.task_id.to_owned()),
                    agent_id: Some(spec.agent_id.to_owned()),
                    session_id: Some("session-local".to_owned()),
                    kind: "log".to_owned(),
                    label: Some(spec.label),
                    summary: Some(spec.summary),
                    uri: None,
                    mime_type: None,
                    payload: serde_json::json!({
                        "decision": format!("{:?}", spec.decision),
                        "request_id": spec.request_id,
                        "at": now_iso(),
                    }),
                },
            )
            .await?;
            store::transition_evidence_status(db, &spec.id, "verified").await?;
            Ok(spec.id)
        }

        // Stage 3 — low-risk read: allowed outright.
        let request = self.echo.plan_action(&agent.id, &task.id, run);
        events.push(RuntimeEvent::ActionProposed {
            request_id: request.id.clone(),
        });
        let evaluation = evaluate_stage(
            engine,
            db,
            &request,
            &policy_broker::PolicyContext::default(),
            "talos-policy-broker/1",
        )
        .await?;
        events.push(RuntimeEvent::PolicyEvaluated {
            request_id: request.id.clone(),
            decision: evaluation.decision,
        });
        let evidence_id = record_verified_evidence(
            db,
            VerifiedEvidence {
                id: format!("diag-ev-{run}-read"),
                task_id: &task.id,
                agent_id: &agent.id,
                label: format!("diagnostic run {run} (read)"),
                summary: evaluation.reason.clone(),
                decision: evaluation.decision,
                request_id: &request.id,
            },
        )
        .await?;
        events.push(RuntimeEvent::EvidenceRecorded {
            evidence_id: evidence_id.clone(),
        });
        events.push(RuntimeEvent::EvidenceVerified {
            evidence_id: evidence_id.clone(),
        });
        evaluations.push(evaluation.clone());
        evidence_ids.push(evidence_id.clone());

        // Stage 4 — write-like: gated behind evidence, then allowed.
        let write_request = self.echo.plan_write_action(&agent.id, &task.id, run);
        events.push(RuntimeEvent::ActionProposed {
            request_id: write_request.id.clone(),
        });
        let gated = evaluate_stage(
            engine,
            db,
            &write_request,
            &policy_broker::PolicyContext::default(),
            "talos-policy-broker/1",
        )
        .await?;
        debug_assert_eq!(gated.decision, PolicyDecision::RequireEvidence);
        events.push(RuntimeEvent::PolicyEvaluated {
            request_id: write_request.id.clone(),
            decision: gated.decision,
        });
        let write_evidence_id = record_verified_evidence(
            db,
            VerifiedEvidence {
                id: format!("diag-ev-{run}-write"),
                task_id: &task.id,
                agent_id: &agent.id,
                label: format!("diagnostic run {run} (write)"),
                summary: gated.reason.clone(),
                decision: gated.decision,
                request_id: &write_request.id,
            },
        )
        .await?;
        events.push(RuntimeEvent::EvidenceRecorded {
            evidence_id: write_evidence_id.clone(),
        });
        events.push(RuntimeEvent::EvidenceVerified {
            evidence_id: write_evidence_id.clone(),
        });
        let write_evaluation = evaluate_stage(
            engine,
            db,
            &write_request,
            &policy_broker::PolicyContext {
                has_evidence: store::has_evidence_for_task(db, &task.id).await?,
                has_compensation_plan: false,
            },
            "talos-policy-broker/1",
        )
        .await?;
        events.push(RuntimeEvent::PolicyEvaluated {
            request_id: write_request.id.clone(),
            decision: write_evaluation.decision,
        });
        evaluations.push(write_evaluation);
        evidence_ids.push(write_evidence_id);

        // Stage 5 — irreversible: gated behind a compensation plan, then allowed.
        let risky_request = self.echo.plan_irreversible_action(&agent.id, &task.id, run);
        events.push(RuntimeEvent::ActionProposed {
            request_id: risky_request.id.clone(),
        });
        let gated_risky = evaluate_stage(
            engine,
            db,
            &risky_request,
            &policy_broker::PolicyContext::default(),
            "talos-policy-broker/1",
        )
        .await?;
        debug_assert_eq!(gated_risky.decision, PolicyDecision::RequireCompensation);
        events.push(RuntimeEvent::PolicyEvaluated {
            request_id: risky_request.id.clone(),
            decision: gated_risky.decision,
        });
        let plan_id = format!("diag-plan-{run}");
        store::create_compensation_plan(
            db,
            store::NewCompensationPlan {
                id: plan_id.clone(),
                action_request_id: risky_request.id.clone(),
                strategy: "rollback_command".to_owned(),
                reason: format!("diagnostic run {run} needs a way back"),
                steps: vec![
                    store::NewCompensationStep {
                        action_type: "rollback_command".to_owned(),
                        payload: serde_json::json!({ "target": risky_request.target }),
                    },
                    store::NewCompensationStep {
                        action_type: "manual_review".to_owned(),
                        payload: serde_json::json!({}),
                    },
                ],
            },
        )
        .await?;
        events.push(RuntimeEvent::CompensationPlanned {
            plan_id: plan_id.clone(),
        });
        let risky_evaluation = evaluate_stage(
            engine,
            db,
            &risky_request,
            &policy_broker::PolicyContext {
                has_evidence: true,
                has_compensation_plan: store::has_compensation_plan(db, &risky_request.id).await?,
            },
            "talos-policy-broker/1",
        )
        .await?;
        events.push(RuntimeEvent::PolicyEvaluated {
            request_id: risky_request.id.clone(),
            decision: risky_evaluation.decision,
        });
        let risky_evidence_id = record_verified_evidence(
            db,
            VerifiedEvidence {
                id: format!("diag-ev-{run}-rotate"),
                task_id: &task.id,
                agent_id: &agent.id,
                label: format!("diagnostic run {run} (rotate)"),
                summary: risky_evaluation.reason.clone(),
                decision: risky_evaluation.decision,
                request_id: &risky_request.id,
            },
        )
        .await?;
        events.push(RuntimeEvent::EvidenceRecorded {
            evidence_id: risky_evidence_id.clone(),
        });
        events.push(RuntimeEvent::EvidenceVerified {
            evidence_id: risky_evidence_id.clone(),
        });
        evaluations.push(risky_evaluation.clone());
        evidence_ids.push(risky_evidence_id);

        // Stage 6 — raise a claim over all three evidence rows and verify it.
        let claim_id = format!("diag-claim-{run}");
        store::create_claim(
            db,
            store::NewClaimPending {
                id: claim_id.clone(),
                statement: format!("diagnostic run {run} completed per policy"),
                created_by_agent_id: Some(agent.id.clone()),
                task_id: Some(task.id.clone()),
            },
        )
        .await?;
        events.push(RuntimeEvent::ClaimCreated {
            claim_id: claim_id.clone(),
        });
        for evidence_id in &evidence_ids {
            store::attach_evidence_to_claim(db, &claim_id, evidence_id).await?;
        }
        let claim_status = store::evaluate_claim_status(db, &claim_id).await?;
        events.push(RuntimeEvent::ClaimEvaluated {
            claim_id: claim_id.clone(),
            status: claim_status,
        });

        // Stage 7 — close out.
        let summary = self.echo.execute_summary(&risky_evaluation);
        task.status = "completed".to_owned();
        agent.status = "completed".to_owned();
        if let Some(stored) = self.state.tasks.iter_mut().find(|t| t.id == task.id) {
            stored.status = task.status.clone();
        }
        if let Some(stored) = self.state.agents.iter_mut().find(|a| a.id == agent.id) {
            stored.status = agent.status.clone();
        }
        events.push(RuntimeEvent::TaskCompleted {
            task_id: task.id.clone(),
            summary,
        });

        Ok(DiagnosticRun {
            task,
            agent,
            request,
            evaluation,
            evidence_id,
            evaluations,
            evidence_ids,
            claim_id,
            plan_id,
            events,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn memory_db() -> evidence_store::EvidenceDb {
        evidence_store::EvidenceDb::connect_memory()
            .await
            .expect("in-memory database connects")
    }

    #[tokio::test]
    async fn orchestrator_creates_task_and_agent() {
        let mut orchestrator = LocalOrchestrator::new();
        let engine = policy_broker::DefaultPolicyEngine::new();
        let db = memory_db().await;
        let run = orchestrator
            .run_diagnostic(&engine, &db)
            .await
            .expect("diagnostic runs");
        assert_eq!(run.task.id, "diag-task-1");
        assert_eq!(run.agent.id, "diag-agent-1");
        assert_eq!(orchestrator.state().tasks.len(), 1);
        assert_eq!(orchestrator.state().agents.len(), 1);
        assert_eq!(orchestrator.state().run_count, 1);
    }

    #[tokio::test]
    async fn echo_action_passes_policy_as_allow() {
        let mut orchestrator = LocalOrchestrator::new();
        let engine = policy_broker::DefaultPolicyEngine::new();
        let db = memory_db().await;
        let run = orchestrator
            .run_diagnostic(&engine, &db)
            .await
            .expect("diagnostic runs");
        assert_eq!(run.evaluation.decision, PolicyDecision::Allow);
        assert_eq!(run.evaluation.request_id, run.request.id);
        // Write stage gates behind evidence, then allows; irreversible
        // stage gates behind a compensation plan, then allows.
        let decisions: Vec<PolicyDecision> = run.evaluations.iter().map(|e| e.decision).collect();
        assert_eq!(
            decisions,
            [
                PolicyDecision::Allow,
                PolicyDecision::Allow,
                PolicyDecision::Allow
            ]
        );
        assert_eq!(run.evaluations.len(), 3);
        // The diagnostic claim links all three verified evidence rows.
        assert_eq!(run.claim_id, "diag-claim-1");
        assert_eq!(run.plan_id, "diag-plan-1");
        assert_eq!(run.evidence_ids.len(), 3);
        let status = evidence_store::evaluate_claim_status(&db, &run.claim_id)
            .await
            .expect("claim evaluates");
        assert_eq!(status, "verified");
    }

    #[tokio::test]
    async fn events_emit_in_lifecycle_order() {
        let mut orchestrator = LocalOrchestrator::new();
        let engine = policy_broker::DefaultPolicyEngine::new();
        let db = memory_db().await;
        let run = orchestrator
            .run_diagnostic(&engine, &db)
            .await
            .expect("diagnostic runs");
        let kinds: Vec<&str> = run
            .events
            .iter()
            .map(|event| match event {
                RuntimeEvent::TaskCreated { .. } => "task_created",
                RuntimeEvent::AgentSpawned { .. } => "agent_spawned",
                RuntimeEvent::ActionProposed { .. } => "action_proposed",
                RuntimeEvent::PolicyEvaluated { .. } => "policy_evaluated",
                RuntimeEvent::EvidenceRecorded { .. } => "evidence_recorded",
                RuntimeEvent::EvidenceVerified { .. } => "evidence_verified",
                RuntimeEvent::ClaimCreated { .. } => "claim_created",
                RuntimeEvent::CompensationPlanned { .. } => "compensation_planned",
                RuntimeEvent::ClaimEvaluated { .. } => "claim_evaluated",
                RuntimeEvent::TaskCompleted { .. } => "task_completed",
                RuntimeEvent::TaskStatusChanged { .. } => "task_status_changed",
            })
            .collect();
        assert_eq!(
            kinds,
            [
                "task_created",
                "agent_spawned",
                "action_proposed",
                "policy_evaluated",
                "evidence_recorded",
                "evidence_verified",
                "action_proposed",
                "policy_evaluated",
                "evidence_recorded",
                "evidence_verified",
                "policy_evaluated",
                "action_proposed",
                "policy_evaluated",
                "compensation_planned",
                "policy_evaluated",
                "evidence_recorded",
                "evidence_verified",
                "claim_created",
                "claim_evaluated",
                "task_completed",
            ]
        );
    }

    #[tokio::test]
    async fn runtime_state_updates_to_completed() {
        let mut orchestrator = LocalOrchestrator::new();
        let engine = policy_broker::DefaultPolicyEngine::new();
        let db = memory_db().await;
        orchestrator
            .run_diagnostic(&engine, &db)
            .await
            .expect("diagnostic runs");
        assert_eq!(orchestrator.state().tasks[0].status, "completed");
        assert_eq!(orchestrator.state().agents[0].status, "completed");
        // Second run keeps deterministic, counter-scoped identifiers.
        let second = orchestrator
            .run_diagnostic(&engine, &db)
            .await
            .expect("second run works");
        assert_eq!(second.task.id, "diag-task-2");
        assert_eq!(orchestrator.state().run_count, 2);
    }

    #[tokio::test]
    async fn lifecycle_valid_and_invalid_transitions() {
        assert!(lifecycle::can_transition("queued", "planning"));
        assert!(lifecycle::can_transition("executing", "waiting_policy"));
        assert!(lifecycle::can_transition("waiting_policy", "executing"));
        assert!(lifecycle::can_transition("executing", "completed"));
        assert!(!lifecycle::can_transition("queued", "completed"));
        assert!(!lifecycle::can_transition("completed", "queued"));
        assert!(lifecycle::is_terminal("completed"));
        assert!(!lifecycle::is_terminal("executing"));
    }

    #[tokio::test]
    async fn queue_submit_execute_quick_success() {
        use crate::queue as q;
        let engine = policy_broker::DefaultPolicyEngine::new();
        let db = memory_db().await;
        let mut seq = 0_u64;
        let view = q::submit_task(
            &db,
            &mut seq,
            q::SubmitParams {
                title: "quick".to_owned(),
                description: None,
                priority: "high".to_owned(),
                timeout_ms: 5_000,
                max_attempts: 3,
                scenario: "quick_success".to_owned(),
            },
        )
        .await
        .expect("submit");
        assert_eq!(view.status, "queued");
        let cancel = q::CancelToken::new();
        let done = q::execute_task(&db, &engine, &view.id, &cancel)
            .await
            .expect("execute");
        assert_eq!(done.status, "completed");
        assert_eq!(done.attempts, 1);
        let transitions = evidence_store::list_transitions(&db, &view.id)
            .await
            .expect("transitions");
        assert!(transitions.len() >= 3);
        assert!(evidence_store::transitions_ledgered(&db)
            .await
            .expect("ledger"));
    }

    #[tokio::test]
    async fn queue_priority_ordering() {
        use crate::queue as q;
        let db = memory_db().await;
        let mut seq = 0_u64;
        for (title, priority) in [("low", "low"), ("crit", "critical"), ("high", "high")] {
            q::submit_task(
                &db,
                &mut seq,
                q::SubmitParams {
                    title: title.to_owned(),
                    description: None,
                    priority: priority.to_owned(),
                    timeout_ms: 5_000,
                    max_attempts: 2,
                    scenario: "quick_success".to_owned(),
                },
            )
            .await
            .expect("submit");
        }
        let queued = q::list_queue(&db, 10).await.expect("queue");
        let order: Vec<&str> = queued.iter().map(|t| t.priority.as_str()).collect();
        assert_eq!(order, ["critical", "high", "low"]);
    }

    #[tokio::test]
    async fn queue_retryable_failure_requeues_then_succeeds() {
        use crate::queue as q;
        let engine = policy_broker::DefaultPolicyEngine::new();
        let db = memory_db().await;
        let mut seq = 0_u64;
        let view = q::submit_task(
            &db,
            &mut seq,
            q::SubmitParams {
                title: "flaky".to_owned(),
                description: None,
                priority: "normal".to_owned(),
                timeout_ms: 5_000,
                max_attempts: 3,
                scenario: "retryable_failure".to_owned(),
            },
        )
        .await
        .expect("submit");
        let cancel = q::CancelToken::new();
        let after_first = q::execute_task(&db, &engine, &view.id, &cancel)
            .await
            .expect("first attempt");
        assert_eq!(after_first.status, "queued");
        let after_second = q::execute_task(&db, &engine, &view.id, &cancel)
            .await
            .expect("second attempt");
        assert_eq!(after_second.status, "completed");
        assert_eq!(after_second.attempts, 2);
    }

    #[tokio::test]
    async fn queue_non_retryable_failure_is_terminal() {
        use crate::queue as q;
        let engine = policy_broker::DefaultPolicyEngine::new();
        let db = memory_db().await;
        let mut seq = 0_u64;
        let view = q::submit_task(
            &db,
            &mut seq,
            q::SubmitParams {
                title: "fatal".to_owned(),
                description: None,
                priority: "normal".to_owned(),
                timeout_ms: 5_000,
                max_attempts: 3,
                scenario: "non_retryable_failure".to_owned(),
            },
        )
        .await
        .expect("submit");
        let done = q::execute_task(&db, &engine, &view.id, &q::CancelToken::new())
            .await
            .expect("execute");
        assert_eq!(done.status, "failed");
        let retried = q::retry_task(&db, &view.id).await.expect("retry");
        assert_eq!(retried.status, "queued");
    }

    #[tokio::test]
    async fn queue_cancellation_propagates() {
        use crate::queue as q;
        let engine = policy_broker::DefaultPolicyEngine::new();
        let db = memory_db().await;
        let mut seq = 0_u64;
        let view = q::submit_task(
            &db,
            &mut seq,
            q::SubmitParams {
                title: "cancel me".to_owned(),
                description: None,
                priority: "normal".to_owned(),
                timeout_ms: 5_000,
                max_attempts: 2,
                scenario: "cancellation_sensitive".to_owned(),
            },
        )
        .await
        .expect("submit");
        q::cancel_task(&db, &view.id).await.expect("cancel");
        let mid = q::get_task_state(&db, &view.id).await.expect("state");
        assert_eq!(mid.status, "canceling");
        let done = q::execute_task(&db, &engine, &view.id, &q::CancelToken::new())
            .await
            .expect("execute");
        assert_eq!(done.status, "canceled");
    }

    #[tokio::test]
    async fn queue_timeout_marks_timed_out() {
        use crate::queue as q;
        let engine = policy_broker::DefaultPolicyEngine::new();
        let db = memory_db().await;
        let mut seq = 0_u64;
        let view = q::submit_task(
            &db,
            &mut seq,
            q::SubmitParams {
                title: "slow".to_owned(),
                description: None,
                priority: "normal".to_owned(),
                timeout_ms: 100,
                max_attempts: 1,
                scenario: "delayed_success".to_owned(),
            },
        )
        .await
        .expect("submit");
        // Delayed scenario sleeps 120ms; timeout is 100ms -> timed_out.
        let done = q::execute_task(&db, &engine, &view.id, &q::CancelToken::new())
            .await
            .expect("execute");
        assert_eq!(done.status, "timed_out");
    }

    #[tokio::test]
    async fn queue_persistence_survives_reload() {
        use crate::queue as q;
        let db = evidence_store::EvidenceDb::connect_memory()
            .await
            .expect("memory db");
        let mut seq = 0_u64;
        let view = q::submit_task(
            &db,
            &mut seq,
            q::SubmitParams {
                title: "durable".to_owned(),
                description: Some("persist me".to_owned()),
                priority: "critical".to_owned(),
                timeout_ms: 5_000,
                max_attempts: 2,
                scenario: "quick_success".to_owned(),
            },
        )
        .await
        .expect("submit");
        // Same pool = same database: state survives across callers.
        let reloaded = q::get_task_state(&db, &view.id).await.expect("reload");
        assert_eq!(reloaded.title, "durable");
        assert_eq!(reloaded.priority, "critical");
        let transitions = evidence_store::list_transitions(&db, &view.id)
            .await
            .expect("history");
        assert!(!transitions.is_empty());
    }

    #[tokio::test]
    async fn mock_scenarios_are_deterministic() {
        let retryable = MockEchoAgent::with_config(EchoConfig {
            scenario: EchoScenario::RetryableFailure,
            seed: 42,
            delay_ms: 0,
        });
        assert_eq!(retryable.outcome_for_attempt(1), (false, true));
        assert_eq!(retryable.outcome_for_attempt(2), (true, false));
        let fatal = MockEchoAgent::with_config(EchoConfig {
            scenario: EchoScenario::NonRetryableFailure,
            seed: 42,
            delay_ms: 0,
        });
        assert_eq!(fatal.outcome_for_attempt(1), (false, false));
    }
}
