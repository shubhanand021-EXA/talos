//! Typed event names mirroring `@talos/contracts` `IpcEventName`.
//!
//! Values are identical to the TypeScript constants so frontend listeners
//! and backend emitters can never drift: `heartbeat`, `taskCreated`,
//! `taskUpdated`, `agentSpawned`, `agentUpdated`, `policyEvaluated`,
//! `evidenceUpdated`, `compensationRequired`, `auditCompleted`,
//! `computerSafetyChanged`, `contextCompacted`, `llmRequestStarted`,
//! `llmRequestCompleted`, `llmBudgetWarning`, `browserWorkerCrashed`.
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
/// Emitted when the browser worker process crashes or times out. The payload
/// carries `{ taskId, toolId, code, checkedAt }`; the tool receipt already
/// marks the task failed.
pub const BROWSER_WORKER_CRASHED: TalosEventName = "browserWorkerCrashed";
/// Emitted when a TypeCompact compaction finishes. The payload carries
/// `{ taskId, evidenceId, proofHash, originalTokens, compactedTokens, checkedAt }`.
pub const CONTEXT_COMPACTED: TalosEventName = "contextCompacted";
/// `llmBudgetWarning` at 80% of cap; `contextBudgetExceeded` on hard-limit failure.
/// The payload carries `{ taskId, totalTokens, contextWindow, checkedAt }`.
pub const CONTEXT_BUDGET_EXCEEDED: TalosEventName = "contextBudgetExceeded";
/// Emitted when a gated model call starts. The payload carries
/// `{ taskId, agentId, providerId, model, checkedAt }` — never prompts.
pub const LLM_REQUEST_STARTED: TalosEventName = "llmRequestStarted";
/// Emitted when a gated model call finishes. The payload carries
/// `{ taskId, providerId, model, totalTokens, estimatedCostUsd, latencyMs,
/// checkedAt }` — metadata only, never content.
pub const LLM_REQUEST_COMPLETED: TalosEventName = "llmRequestCompleted";
/// Emitted when a capped task crosses 80% of its spend. The payload carries
/// `{ taskId, spentUsd, budgetUsd, checkedAt }`.
pub const LLM_BUDGET_WARNING: TalosEventName = "llmBudgetWarning";
/// Emitted when the Supervisor delegates a sub-task. The payload carries
/// `{ parentTaskId, childTaskId, role, checkedAt }`.
pub const AGENT_DELEGATED: TalosEventName = "agentDelegated";
/// Emitted when a delegated child task spawns. The payload carries
/// `{ parentTaskId, childTaskId, role }`.
pub const CHILD_TASK_SPAWNED: TalosEventName = "childTaskSpawned";
/// Emitted when a delegated child completes. The payload carries
/// `{ parentTaskId, childTaskId, status }`.
pub const CHILD_TASK_COMPLETED: TalosEventName = "childTaskCompleted";
/// Emitted when a delegated child fails. The payload carries
/// `{ parentTaskId, childTaskId, error }`.
pub const CHILD_TASK_FAILED: TalosEventName = "childTaskFailed";
/// Emitted when child results merge into a parent. The payload carries
/// `{ parentTaskId, status, artifactCount }`.
pub const RESULTS_MERGED: TalosEventName = "resultsMerged";
/// Emitted when a coding workspace is created, cleaned, or used. The payload
/// carries `{ taskId, path, status, checkedAt }`.
pub const WORKSPACE_UPDATED: TalosEventName = "workspaceUpdated";
/// Emitted after a hybrid memory search. The payload carries
/// `{ queryChars, resultCount, mode, checkedAt }` — counts only, never text.
pub const MEMORY_INDEXED: TalosEventName = "memoryIndexed";
/// Emitted when a quarantined skill is proposed. The payload carries
/// `{ skillId, status, checkedAt }`.
pub const SKILL_CREATED: TalosEventName = "skillCreated";
/// Emitted after a full memory reindex. The payload carries
/// `{ entriesAdded, checkedAt }`.
pub const INDEX_REBUILT: TalosEventName = "indexRebuilt";
/// Emitted when policy escalates to human review. The payload carries
/// `{ approvalId, taskId, ...approval }` — the full ApprovalSheet record.
pub const APPROVAL_REQUESTED: TalosEventName = "approvalRequested";
/// Emitted when a human grants an approval. The payload carries
/// `{ approvalId, taskId, grantedBy, grantedAt }`.
pub const APPROVAL_GRANTED: TalosEventName = "approvalGranted";
/// Emitted when a human denies an approval. The payload carries
/// `{ approvalId, taskId, deniedBy, reason }`.
pub const APPROVAL_DENIED: TalosEventName = "approvalDenied";
/// Emitted when an approval times out. The payload carries
/// `{ approvalId, taskId }`.
pub const APPROVAL_EXPIRED: TalosEventName = "approvalExpired";
/// Emitted when a compensation execution finishes a step or completes.
/// The payload carries `{ planId, taskId, stepsCompleted, status }`.
pub const COMPENSATION_EXECUTED: TalosEventName = "compensationExecuted";

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
        assert_eq!(BROWSER_WORKER_CRASHED, "browserWorkerCrashed");
        assert_eq!(CONTEXT_COMPACTED, "contextCompacted");
        assert_eq!(CONTEXT_BUDGET_EXCEEDED, "contextBudgetExceeded");
        assert_eq!(LLM_REQUEST_STARTED, "llmRequestStarted");
        assert_eq!(LLM_REQUEST_COMPLETED, "llmRequestCompleted");
        assert_eq!(LLM_BUDGET_WARNING, "llmBudgetWarning");
        assert_eq!(AGENT_DELEGATED, "agentDelegated");
        assert_eq!(CHILD_TASK_SPAWNED, "childTaskSpawned");
        assert_eq!(CHILD_TASK_COMPLETED, "childTaskCompleted");
        assert_eq!(CHILD_TASK_FAILED, "childTaskFailed");
        assert_eq!(RESULTS_MERGED, "resultsMerged");
        assert_eq!(WORKSPACE_UPDATED, "workspaceUpdated");
        assert_eq!(MEMORY_INDEXED, "memoryIndexed");
        assert_eq!(SKILL_CREATED, "skillCreated");
        assert_eq!(INDEX_REBUILT, "indexRebuilt");
        assert_eq!(APPROVAL_REQUESTED, "approvalRequested");
        assert_eq!(APPROVAL_GRANTED, "approvalGranted");
        assert_eq!(APPROVAL_DENIED, "approvalDenied");
        assert_eq!(APPROVAL_EXPIRED, "approvalExpired");
        assert_eq!(COMPENSATION_EXECUTED, "compensationExecuted");
    }
}
