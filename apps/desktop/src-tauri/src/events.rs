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
/// Emitted when the browser agent pauses for a human (CAPTCHA / login wall).
/// The payload carries `{ interventionId, taskId, kind, url?, reason,
/// checkedAt }` — linkage and reason only, never page content. The UI renders
/// the intervention modal over the foreground Playwright window; the human
/// resolves it with `resumeBrowserHitl`.
pub const BROWSER_HITL_REQUIRED: TalosEventName = "browserHitlRequired";
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
/// Emitted when a skill moves `quarantined` → `validated`. The payload
/// carries `{ skillId, status, checkedAt }`.
pub const SKILL_VALIDATED: TalosEventName = "skillValidated";
/// Emitted when a skill moves `validated` → `published` and becomes
/// retrievable in context assembly. The payload carries
/// `{ skillId, status, checkedAt }`.
pub const SKILL_PUBLISHED: TalosEventName = "skillPublished";
/// Emitted when a skill moves `published` → `deprecated`. The payload
/// carries `{ skillId, status, checkedAt }`.
pub const SKILL_DEPRECATED: TalosEventName = "skillDeprecated";
/// Emitted when a skill is archived (terminal). The payload carries
/// `{ skillId, status, checkedAt }`.
pub const SKILL_ARCHIVED: TalosEventName = "skillArchived";
/// Emitted when a GEPA evaluation pass completes. The payload carries
/// `{ patternsFound, skillsProposed, skillsUsed, skillsDeprecated,
/// needsReview, evidenceId, checkedAt }` — counts and ids only.
pub const SKILL_EVALUATION_COMPLETED: TalosEventName = "skillEvaluationCompleted";
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
/// Emitted when a real computer-use action's verification fails (the frame
/// did not change as targeted). The payload carries
/// `{ taskId, toolId, beforeHash, afterHash, detail, checkedAt }`.
pub const ACTION_VERIFICATION_FAILED: TalosEventName = "actionVerificationFailed";
/// Emitted when a real computer-use action loses input focus unexpectedly.
/// The payload carries `{ taskId, toolId, checkedAt }`.
pub const COMPUTER_FOCUS_LOST: TalosEventName = "computerFocusLost";
/// Emitted when a storage backup snapshot is created. The payload carries
/// `{ path, sizeBytes, backupCount, checkedAt }` — paths only, never content.
pub const BACKUP_CREATED: TalosEventName = "backupCreated";
/// Emitted when a storage backup is restored. The payload carries
/// `{ path, tablesCopied, chainValid, chainChecked, checkedAt }`.
pub const BACKUP_RESTORED: TalosEventName = "backupRestored";
/// Emitted when a child's spend exceeds its budget reservation. The payload
/// carries `{ childTaskId, reservedUsd, spentUsd, checkedAt }`.
pub const CHILD_BUDGET_EXCEEDED: TalosEventName = "childBudgetExceeded";
/// Emitted when the Supervisor produces a DAG plan. The payload carries
/// `{ taskId, method, stepCount, checkedAt }`.
pub const PLAN_GENERATED: TalosEventName = "planGenerated";
/// Emitted when a DAG plan passes every deterministic gate. The payload
/// carries `{ taskId, levels, requiresApproval, checkedAt }`.
pub const PLAN_VALIDATED: TalosEventName = "planValidated";
/// Emitted when a DAG plan is rejected by a deterministic gate. The payload
/// carries `{ taskId, code, reason, checkedAt }`.
pub const PLAN_REJECTED: TalosEventName = "planRejected";
/// Emitted when planning falls back to keyword decomposition. The payload
/// carries `{ taskId, reason, checkedAt }`.
pub const PLAN_FALLBACK: TalosEventName = "planFallback";
/// Emitted when a tool runs inside an OS sandbox. The payload carries
/// `{ taskId, toolId, profile, enforcementMethod, checkedAt }`.
pub const SANDBOX_ENFORCED: TalosEventName = "sandboxEnforced";
/// Emitted when an MCP server completes its handshake. The payload carries
/// `{ server, toolCount, checkedAt }`.
pub const MCP_SERVER_CONNECTED: TalosEventName = "mcpServerConnected";
/// Emitted when an MCP server is removed or its transport fails. The payload
/// carries `{ server, checkedAt }`.
pub const MCP_SERVER_DISCONNECTED: TalosEventName = "mcpServerDisconnected";
/// Emitted after every MCP tool call (allowed or denied). The payload
/// carries `{ server, tool, decision, checkedAt }` — never arguments.
pub const MCP_TOOL_CALL_COMPLETED: TalosEventName = "mcpToolCallCompleted";
/// Emitted after an instruction file parses. The payload carries
/// `{ path, kind, checkedAt }` — summaries only, never secrets.
pub const INSTRUCTION_FILE_PARSED: TalosEventName = "instructionFileParsed";
/// Emitted when a new plugin needs permission review. The payload carries
/// `{ plugin, requested, capabilities, unsigned, checkedAt }`.
pub const PLUGIN_APPROVAL_REQUESTED: TalosEventName = "pluginApprovalRequested";
/// Emitted when sandbox enforcement denies or reports a violation. The payload
/// carries `{ taskId, toolId?, profile?, code, detail, checkedAt }`.
pub const SANDBOX_VIOLATION: TalosEventName = "sandboxViolation";
/// Emitted when a storage integrity check completes. The payload carries
/// `{ valid, checked, firstBadSeq, checkedAt }`.
pub const INTEGRITY_CHECK_COMPLETED: TalosEventName = "integrityCheckCompleted";

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
        assert_eq!(BROWSER_HITL_REQUIRED, "browserHitlRequired");
        assert_eq!(CONTEXT_COMPACTED, "contextCompacted");
        assert_eq!(CONTEXT_BUDGET_EXCEEDED, "contextBudgetExceeded");
        assert_eq!(LLM_REQUEST_STARTED, "llmRequestStarted");
        assert_eq!(LLM_REQUEST_COMPLETED, "llmRequestCompleted");
        assert_eq!(LLM_BUDGET_WARNING, "llmBudgetWarning");
        assert_eq!(AGENT_DELEGATED, "agentDelegated");
        assert_eq!(CHILD_TASK_SPAWNED, "childTaskSpawned");
        assert_eq!(CHILD_TASK_COMPLETED, "childTaskCompleted");
        assert_eq!(CHILD_TASK_FAILED, "childTaskFailed");
        assert_eq!(CHILD_BUDGET_EXCEEDED, "childBudgetExceeded");
        assert_eq!(PLAN_GENERATED, "planGenerated");
        assert_eq!(PLAN_VALIDATED, "planValidated");
        assert_eq!(PLAN_REJECTED, "planRejected");
        assert_eq!(PLAN_FALLBACK, "planFallback");
        assert_eq!(SANDBOX_ENFORCED, "sandboxEnforced");
        assert_eq!(SANDBOX_VIOLATION, "sandboxViolation");
        assert_eq!(MCP_SERVER_CONNECTED, "mcpServerConnected");
        assert_eq!(MCP_SERVER_DISCONNECTED, "mcpServerDisconnected");
        assert_eq!(MCP_TOOL_CALL_COMPLETED, "mcpToolCallCompleted");
        assert_eq!(INSTRUCTION_FILE_PARSED, "instructionFileParsed");
        assert_eq!(PLUGIN_APPROVAL_REQUESTED, "pluginApprovalRequested");
        assert_eq!(RESULTS_MERGED, "resultsMerged");
        assert_eq!(WORKSPACE_UPDATED, "workspaceUpdated");
        assert_eq!(MEMORY_INDEXED, "memoryIndexed");
        assert_eq!(SKILL_CREATED, "skillCreated");
        assert_eq!(SKILL_VALIDATED, "skillValidated");
        assert_eq!(SKILL_PUBLISHED, "skillPublished");
        assert_eq!(SKILL_DEPRECATED, "skillDeprecated");
        assert_eq!(SKILL_ARCHIVED, "skillArchived");
        assert_eq!(SKILL_EVALUATION_COMPLETED, "skillEvaluationCompleted");
        assert_eq!(INDEX_REBUILT, "indexRebuilt");
        assert_eq!(APPROVAL_REQUESTED, "approvalRequested");
        assert_eq!(APPROVAL_GRANTED, "approvalGranted");
        assert_eq!(APPROVAL_DENIED, "approvalDenied");
        assert_eq!(APPROVAL_EXPIRED, "approvalExpired");
        assert_eq!(COMPENSATION_EXECUTED, "compensationExecuted");
        assert_eq!(ACTION_VERIFICATION_FAILED, "actionVerificationFailed");
        assert_eq!(COMPUTER_FOCUS_LOST, "computerFocusLost");
        assert_eq!(BACKUP_CREATED, "backupCreated");
        assert_eq!(BACKUP_RESTORED, "backupRestored");
        assert_eq!(INTEGRITY_CHECK_COMPLETED, "integrityCheckCompleted");
    }
}
