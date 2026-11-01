//! Typed inter-agent message passing (Supervisor <-> specialists).
//!
//! No direct agent-to-agent communication: every message flows through the
//! Supervisor over bounded [`tokio::sync::mpsc`] channels. All messages are
//! typed, validated, and (by the caller) logged as `delegation.*` events in
//! the evidence store.

use talos_common::{Result, TalosError};

use crate::delegation::ArtifactRef;

/// Supervisor → agent directives.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SupervisorToAgent {
    /// Assign a scoped sub-task to a specialist.
    TaskAssignment {
        /// Child task the specialist owns.
        child_task_id: String,
        /// Bounded scope (never the full parent goal).
        scope: String,
        /// Tools the child may invoke.
        tool_allowlist: Vec<String>,
        /// Isolated context window (tokens).
        context_budget: u32,
        /// Per-child deadline (milliseconds).
        deadline_ms: i64,
    },
    /// Cancel a running child (propagated from parent cancellation).
    CancelSignal {
        /// Child task to cancel.
        child_task_id: String,
        /// Human reason.
        reason: String,
    },
    /// Update a child's budget mid-flight (Supervisor-owned split).
    BudgetUpdate {
        /// Child task to update.
        child_task_id: String,
        /// New context window (tokens).
        context_budget: u32,
        /// New deadline (milliseconds).
        deadline_ms: i64,
    },
}

/// Agent → Supervisor reports.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentToSupervisor {
    /// Heartbeat with completion fraction (0.0..=1.0).
    ProgressUpdate {
        /// Reporting child.
        child_task_id: String,
        /// Fraction complete.
        progress: f64,
        /// One-line note.
        note: String,
    },
    /// A structured output is ready (not a conversation dump).
    ArtifactProduced {
        /// Reporting child.
        child_task_id: String,
        /// The artifact.
        artifact: ArtifactRef,
        /// Backing evidence row, if persisted.
        evidence_id: Option<String>,
    },
    /// Child finished successfully.
    TaskCompleted {
        /// Finished child.
        child_task_id: String,
        /// Summary line.
        summary: String,
    },
    /// Child failed (retry, reassign, or escalate per Supervisor policy).
    TaskFailed {
        /// Failed child.
        child_task_id: String,
        /// Machine-readable error.
        error: String,
        /// Whether the Supervisor may retry this slice.
        retryable: bool,
    },
}

fn err(msg: &str) -> TalosError {
    TalosError::Storage(msg.to_owned())
}

impl SupervisorToAgent {
    /// Validate field bounds (ids 1..=128, scopes non-empty, budgets sane).
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::TaskAssignment {
                child_task_id,
                scope,
                tool_allowlist,
                context_budget,
                deadline_ms,
            } => {
                if child_task_id.trim().is_empty() || child_task_id.len() > 128 {
                    return Err(err("child_task_id must be 1..=128 chars"));
                }
                if scope.trim().is_empty() || scope.len() > 2048 {
                    return Err(err("scope must be 1..=2048 chars"));
                }
                if tool_allowlist.is_empty() || tool_allowlist.len() > 32 {
                    return Err(err("tool_allowlist must be 1..=32 tools"));
                }
                if *context_budget == 0 || *context_budget > 128_000 {
                    return Err(err("context_budget out of range"));
                }
                if !(100..=3_600_000).contains(deadline_ms) {
                    return Err(err("deadline_ms out of range"));
                }
                Ok(())
            }
            Self::CancelSignal {
                child_task_id,
                reason,
            } => {
                if child_task_id.trim().is_empty() || child_task_id.len() > 128 {
                    return Err(err("child_task_id must be 1..=128 chars"));
                }
                if reason.trim().is_empty() || reason.len() > 1024 {
                    return Err(err("reason must be 1..=1024 chars"));
                }
                Ok(())
            }
            Self::BudgetUpdate {
                child_task_id,
                context_budget,
                deadline_ms,
            } => {
                if child_task_id.trim().is_empty() || child_task_id.len() > 128 {
                    return Err(err("child_task_id must be 1..=128 chars"));
                }
                if *context_budget == 0 || *context_budget > 128_000 {
                    return Err(err("context_budget out of range"));
                }
                if !(100..=3_600_000).contains(deadline_ms) {
                    return Err(err("deadline_ms out of range"));
                }
                Ok(())
            }
        }
    }
}

impl AgentToSupervisor {
    /// Validate field bounds.
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::ProgressUpdate {
                child_task_id,
                progress,
                note,
            } => {
                if child_task_id.trim().is_empty() || child_task_id.len() > 128 {
                    return Err(err("child_task_id must be 1..=128 chars"));
                }
                if !(0.0..=1.0).contains(progress) {
                    return Err(err("progress must be 0.0..=1.0"));
                }
                if note.len() > 1024 {
                    return Err(err("note must be <=1024 chars"));
                }
                Ok(())
            }
            Self::ArtifactProduced {
                child_task_id,
                artifact,
                ..
            } => {
                if child_task_id.trim().is_empty() || child_task_id.len() > 128 {
                    return Err(err("child_task_id must be 1..=128 chars"));
                }
                if artifact.id.trim().is_empty() || artifact.id.len() > 128 {
                    return Err(err("artifact id must be 1..=128 chars"));
                }
                Ok(())
            }
            Self::TaskCompleted {
                child_task_id,
                summary,
            } => {
                if child_task_id.trim().is_empty() || child_task_id.len() > 128 {
                    return Err(err("child_task_id must be 1..=128 chars"));
                }
                if summary.trim().is_empty() || summary.len() > 2048 {
                    return Err(err("summary must be 1..=2048 chars"));
                }
                Ok(())
            }
            Self::TaskFailed {
                child_task_id,
                error,
                ..
            } => {
                if child_task_id.trim().is_empty() || child_task_id.len() > 128 {
                    return Err(err("child_task_id must be 1..=128 chars"));
                }
                if error.trim().is_empty() || error.len() > 2048 {
                    return Err(err("error must be 1..=2048 chars"));
                }
                Ok(())
            }
        }
    }
}

/// Bounded Supervisor<->agent bus (no direct agent-to-agent traffic).
#[derive(Debug)]
pub struct DelegationBus {
    /// Supervisor → agent sender.
    pub supervisor_tx: tokio::sync::mpsc::Sender<SupervisorToAgent>,
    /// Supervisor → agent receiver (held by the specialist loop).
    pub agent_rx: tokio::sync::mpsc::Receiver<SupervisorToAgent>,
    /// Agent → Supervisor sender.
    pub agent_tx: tokio::sync::mpsc::Sender<AgentToSupervisor>,
    /// Agent → Supervisor receiver (held by the Supervisor loop).
    pub supervisor_rx: tokio::sync::mpsc::Receiver<AgentToSupervisor>,
}

impl DelegationBus {
    /// Create a bounded bus (capacity applies to each direction).
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        let (supervisor_tx, agent_rx) = tokio::sync::mpsc::channel(capacity.max(1));
        let (agent_tx, supervisor_rx) = tokio::sync::mpsc::channel(capacity.max(1));
        Self {
            supervisor_tx,
            agent_rx,
            agent_tx,
            supervisor_rx,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assignment_validates_bounds() {
        let good = SupervisorToAgent::TaskAssignment {
            child_task_id: "child-1".to_owned(),
            scope: "code slice".to_owned(),
            tool_allowlist: vec!["fs.read".to_owned()],
            context_budget: 4000,
            deadline_ms: 30_000,
        };
        assert!(good.validate().is_ok());
        let bad = SupervisorToAgent::TaskAssignment {
            child_task_id: String::new(),
            scope: "s".to_owned(),
            tool_allowlist: vec!["fs.read".to_owned()],
            context_budget: 4000,
            deadline_ms: 30_000,
        };
        assert!(bad.validate().is_err());
    }

    #[test]
    fn progress_rejects_out_of_range() {
        let bad = AgentToSupervisor::ProgressUpdate {
            child_task_id: "c".to_owned(),
            progress: 2.0,
            note: "x".to_owned(),
        };
        assert!(bad.validate().is_err());
    }

    #[tokio::test]
    async fn bus_delivers_both_directions() {
        let mut bus = DelegationBus::new(8);
        let assignment = SupervisorToAgent::TaskAssignment {
            child_task_id: "child-9".to_owned(),
            scope: "research slice".to_owned(),
            tool_allowlist: vec!["fs.read".to_owned()],
            context_budget: 4000,
            deadline_ms: 30_000,
        };
        assignment.validate().expect("valid");
        bus.supervisor_tx
            .send(assignment.clone())
            .await
            .expect("send");
        let got = bus.agent_rx.recv().await.expect("recv");
        assert_eq!(got, assignment);
        let report = AgentToSupervisor::TaskCompleted {
            child_task_id: "child-9".to_owned(),
            summary: "done".to_owned(),
        };
        report.validate().expect("valid");
        bus.agent_tx.send(report.clone()).await.expect("send");
        let back = bus.supervisor_rx.recv().await.expect("recv");
        assert_eq!(back, report);
    }
}
