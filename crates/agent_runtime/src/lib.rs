//! Local agent runtime: deterministic in-memory orchestration for Batch 1.
//!
//! [`LocalOrchestrator`] coordinates one diagnostic loop with no network and
//! no LLMs: create task → spawn agent → propose action → policy verdict →
//! record evidence → complete task. Every stage appends a [`RuntimeEvent`]
//! that the Tauri layer maps onto `@talos/contracts` event names.

use policy_broker::{
    ActionRequest, Capability, PolicyDecision, PolicyEngine, PolicyEvaluation, RiskLevel,
};
use talos_common::{now_iso, Result};

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
    /// The task reached a terminal state.
    TaskCompleted {
        /// Task identifier.
        task_id: String,
        /// One-line outcome.
        summary: String,
    },
}

/// Full result of one diagnostic run (returned to the IPC layer).
#[derive(Debug, Clone)]
pub struct DiagnosticRun {
    /// Task created for this run.
    pub task: RuntimeTask,
    /// Agent spawned for this run.
    pub agent: RuntimeAgent,
    /// Action the echo agent proposed.
    pub request: ActionRequest,
    /// Policy verdict for the proposal.
    pub evaluation: PolicyEvaluation,
    /// Evidence row recorded for the verdict.
    pub evidence_id: String,
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

/// Deterministic echo agent: no LLM, no network. Accepts a diagnostic task
/// and always proposes the same low-risk read, so policy verdicts are stable.
#[derive(Debug, Default)]
pub struct MockEchoAgent;

impl MockEchoAgent {
    /// Create the echo agent.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Propose the canonical diagnostic action for `task_id`.
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
}

impl LocalOrchestrator {
    /// Create an empty orchestrator.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: RuntimeState::default(),
            echo: MockEchoAgent::new(),
        }
    }

    /// Borrow the current runtime state (tasks, agents, run count).
    #[must_use]
    pub fn state(&self) -> &RuntimeState {
        &self.state
    }

    /// Run one deterministic diagnostic loop against `engine` + `db`.
    ///
    /// Identifiers embed the 1-based run number, so repeated runs are
    /// stable apart from timestamps and the counter itself.
    pub async fn run_diagnostic(
        &mut self,
        engine: &dyn PolicyEngine,
        db: &evidence_store::EvidenceDb,
    ) -> Result<DiagnosticRun> {
        self.state.run_count += 1;
        let run = self.state.run_count;
        let mut events = Vec::new();

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

        // Stage 3 — propose + evaluate.
        let request = self.echo.plan_action(&agent.id, &task.id, run);
        events.push(RuntimeEvent::ActionProposed {
            request_id: request.id.clone(),
        });
        let evaluation = engine.evaluate(&request);
        events.push(RuntimeEvent::PolicyEvaluated {
            request_id: request.id.clone(),
            decision: evaluation.decision,
        });

        // Stage 4 — record the verdict as evidence (append-only).
        let evidence_id = format!("diag-ev-{run}");
        evidence_store::insert_evidence(
            db,
            evidence_store::NewEvidence {
                id: evidence_id.clone(),
                task_id: Some(task.id.clone()),
                agent_id: Some(agent.id.clone()),
                session_id: Some("session-local".to_owned()),
                kind: "log".to_owned(),
                label: Some(format!("diagnostic run {run}")),
                summary: Some(evaluation.reason.clone()),
                uri: None,
                mime_type: None,
                payload: serde_json::json!({
                    "decision": format!("{:?}", evaluation.decision),
                    "request_id": request.id,
                    "at": now_iso(),
                }),
            },
        )
        .await?;
        events.push(RuntimeEvent::EvidenceRecorded {
            evidence_id: evidence_id.clone(),
        });

        // Stage 5 — close out.
        let summary = self.echo.execute_summary(&evaluation);
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
                RuntimeEvent::TaskCompleted { .. } => "task_completed",
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
}
