//! Typed event names mirroring `@talos/contracts` `IpcEventName`.
//!
//! Values are identical to the TypeScript constants so frontend listeners
//! and backend emitters can never drift: `heartbeat`, `taskCreated`,
//! `taskUpdated`, `agentSpawned`, `agentUpdated`, `policyEvaluated`,
//! `evidenceUpdated`, `compensationRequired`, `auditCompleted`,
//! `computerSafetyChanged`.
//!
//! Orchestration arrives in later batches; until then this module is
//! foundation-only: [`emit_talos_event`] is exercised by every diagnostic.
use tauri::Emitter;

/// A Talos event name. Always one of the constants below.
pub type TalosEventName = &'static str;

/// Emitted on the heartbeat channel.
pub const HEARTBEAT: TalosEventName = "heartbeat";
/// Emitted when a task is created.
pub const TASK_CREATED: TalosEventName = "taskCreated";
/// Emitted when a task changes.
pub const TASK_UPDATED: TalosEventName = "taskUpdated";
/// Emitted when an agent spawns.
pub const AGENT_SPAWNED: TalosEventName = "agentSpawned";
/// Emitted when an agent changes (wired by later orchestration batches).
#[allow(dead_code)]
pub const AGENT_UPDATED: TalosEventName = "agentUpdated";
/// Emitted after every policy evaluation.
pub const POLICY_EVALUATED: TalosEventName = "policyEvaluated";
/// Emitted when evidence lands.
pub const EVIDENCE_UPDATED: TalosEventName = "evidenceUpdated";
/// Emitted when an action needs a compensation plan (wired by later batches).
#[allow(dead_code)]
pub const COMPENSATION_REQUIRED: TalosEventName = "compensationRequired";
/// Emitted when an audit completes (wired by later batches).
#[allow(dead_code)]
pub const AUDIT_COMPLETED: TalosEventName = "auditCompleted";
/// Emitted when the computer-use safety state changes (emergency stop engaged
/// or cleared). The payload carries `{ emergencyStopped, canceledTasks, checkedAt }`.
pub const COMPUTER_SAFETY_CHANGED: TalosEventName = "computerSafetyChanged";

/// Emit a typed Talos event to all frontend listeners.
///
/// Orchestration arrives later; this helper guarantees name/payload shape now.
pub fn emit_talos_event(
    app: &tauri::AppHandle,
    event: TalosEventName,
    payload: serde_json::Value,
) -> talos_common::Result<()> {
    app.emit(event, payload)
        .map_err(|err| talos_common::TalosError::Internal(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_names_match_contracts() {
        // Must stay identical to @talos/contracts IpcEventName values.
        assert_eq!(HEARTBEAT, "heartbeat");
        assert_eq!(TASK_CREATED, "taskCreated");
        assert_eq!(TASK_UPDATED, "taskUpdated");
        assert_eq!(AGENT_SPAWNED, "agentSpawned");
        assert_eq!(AGENT_UPDATED, "agentUpdated");
        assert_eq!(POLICY_EVALUATED, "policyEvaluated");
        assert_eq!(EVIDENCE_UPDATED, "evidenceUpdated");
        assert_eq!(COMPENSATION_REQUIRED, "compensationRequired");
        assert_eq!(AUDIT_COMPLETED, "auditCompleted");
        assert_eq!(COMPUTER_SAFETY_CHANGED, "computerSafetyChanged");
    }
}
