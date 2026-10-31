import { z } from "zod";
import {
  ActionRequestSchema,
  ContextBudgetStatusSchema,
  ContextCompactionReceiptSchema,
  HeartbeatEventSchema,
  PolicyEvaluationSchema,
  SubmitTaskInputSchema,
  SystemStatusSchema,
  TaskQueueEntrySchema,
  ToolCandidateSchema,
  ToolIdSchema,
  ToolInvocationSchema,
  ToolManifestSchema,
  ToolReceiptSchema,
  ToolRagQuerySchema,
  type ActionRequest,
  type ContextBudgetStatus,
  type ContextCompactionReceipt,
  type HeartbeatEvent,
  type PolicyEvaluation,
  type SubmitTaskInput,
  type SystemStatus,
  type TaskQueueEntry,
  type ToolCandidate,
  type ToolId,
  type ToolInvocation,
  type ToolManifest,
  type ToolReceipt,
  type ToolRagQuery,
} from "@talos/contracts";
import { invokeCommand } from "./client.js";

/** Product metadata returned by `get_app_info`. */
export const AppInfoSchema = z
  .object({
    name: z.string().min(1).max(64),
    version: z.string().min(1).max(64),
    environment: z.enum(["local", "staging", "production"]),
  })
  .strict();
export type BackendAppInfo = z.infer<typeof AppInfoSchema>;

/** Chained event row for the Effects panel. */
export const EventSummarySchema = z
  .object({
    id: z.string().min(1).max(128),
    kind: z.string().min(1).max(128),
    seq: z.number().int().nonnegative(),
    created_at: z.string().min(1),
  })
  .strict();

/** Evidence row for the Effects panel. */
export const EvidenceSummarySchema = z
  .object({
    id: z.string().min(1).max(128),
    kind: z.string().min(1).max(128),
    status: z.string().min(1).max(64),
    hash: z.string().min(1).max(256),
    created_at: z.string().min(1),
  })
  .strict();

/** Claim row for the Effects panel. */
export const ClaimSummarySchema = z
  .object({
    id: z.string().min(1).max(128),
    statement: z.string().min(1),
    status: z.string().min(1).max(64),
    created_at: z.string().min(1),
  })
  .strict();

/** Compensation plan row for the Effects panel. */
export const CompensationPlanSummarySchema = z
  .object({
    id: z.string().min(1).max(128),
    action_request_id: z.string().min(1).max(128),
    strategy: z.string().min(1).max(128),
    status: z.string().min(1).max(64),
    updated_at: z.string().min(1),
  })
  .strict();
export type EventSummary = z.infer<typeof EventSummarySchema>;
export type EvidenceSummary = z.infer<typeof EvidenceSummarySchema>;
export type ClaimSummary = z.infer<typeof ClaimSummarySchema>;
export type CompensationPlanSummary = z.infer<
  typeof CompensationPlanSummarySchema
>;

/** Ledger snapshot returned by `verify_evidence_ledger`. */
export const LedgerStatusSchema = z
  .object({
    healthy: z.boolean(),
    event_count: z.number().int().nonnegative(),
    evidence_count: z.number().int().nonnegative(),
    claim_count: z.number().int().nonnegative(),
    detail: z.string().max(1024),
    checked_at: z.string().min(1),
    chain_valid: z.boolean(),
    chain_checked: z.number().int().nonnegative(),
    chain_last_seq: z.number().int().nonnegative().nullish(),
    chain_last_hash: z.string().max(256).nullish(),
    chain_first_bad_seq: z.number().int().nonnegative().nullish(),
    recent_events: z.array(EventSummarySchema).max(100),
    recent_evidence: z.array(EvidenceSummarySchema).max(100),
    recent_claims: z.array(ClaimSummarySchema).max(100),
    compensation_plans: z.array(CompensationPlanSummarySchema).max(100),
  })
  .strict();
export type LedgerStatus = z.infer<typeof LedgerStatusSchema>;

/** One step of the diagnostic run. */
export const DiagnosticCheckSchema = z
  .object({
    name: z.string().min(1).max(128),
    ok: z.boolean(),
    detail: z.string().max(2048),
  })
  .strict();

/** Summary returned by `run_diagnostic`. */
export const DiagnosticSummarySchema = z
  .object({
    total: z.number().int().nonnegative(),
    passed: z.number().int().nonnegative(),
    checks: z.array(DiagnosticCheckSchema).max(64),
    ledger: LedgerStatusSchema,
    task_id: z.string().min(1).max(128),
    agent_id: z.string().min(1).max(128),
    evaluation: PolicyEvaluationSchema,
    claim_id: z.string().min(1).max(128),
    plan_id: z.string().min(1).max(128),
  })
  .strict();
export type DiagnosticSummary = z.infer<typeof DiagnosticSummarySchema>;

/** Fetch static product metadata. */
export function getAppInfo(): Promise<BackendAppInfo> {
  return invokeCommand("get_app_info", undefined, AppInfoSchema);
}

/** Fetch the current system health snapshot. */
export function getSystemStatus(): Promise<SystemStatus> {
  return invokeCommand("get_system_status", undefined, SystemStatusSchema);
}

/**
 * Submit one action for policy evaluation. Input is validated against the
 * contracts schema first; malformed input never reaches the backend.
 */
export function requestAction(input: ActionRequest): Promise<PolicyEvaluation> {
  const parsed = ActionRequestSchema.safeParse(input);
  if (!parsed.success) {
    return Promise.reject(
      new Error(`Invalid action request: ${parsed.error.message}`),
    );
  }
  return invokeCommand(
    "request_action",
    { raw: parsed.data },
    PolicyEvaluationSchema,
  );
}

/** Run the local echo-agent diagnostic loop. */
export function runDiagnostic(): Promise<DiagnosticSummary> {
  return invokeCommand("run_diagnostic", undefined, DiagnosticSummarySchema);
}

/** Verify ledger integrity (counts + hash probe). */
export function verifyEvidenceLedger(): Promise<LedgerStatus> {
  return invokeCommand("verify_evidence_ledger", undefined, LedgerStatusSchema);
}

/** Durable queue entry (re-exported contracts schema for the UI). */
export { SubmitTaskInputSchema, TaskQueueEntrySchema };
export type { SubmitTaskInput, TaskQueueEntry };

/** Submit a durable task; the backend executes it through the policy broker. */
export function submitTask(input: SubmitTaskInput): Promise<TaskQueueEntry> {
  const parsed = SubmitTaskInputSchema.safeParse(input);
  if (!parsed.success) {
    return Promise.reject(
      new Error(`Invalid submit_task input: ${parsed.error.message}`),
    );
  }
  return invokeCommand(
    "submit_task",
    { input: parsed.data },
    TaskQueueEntrySchema,
  );
}

/** Request cancellation for one task. */
export function cancelTask(taskId: string): Promise<TaskQueueEntry> {
  return invokeCommand(
    "cancel_task",
    { task_id: taskId },
    TaskQueueEntrySchema,
  );
}

/** Retry a finished task (failed / timed_out / canceled -> queued). */
export function retryTask(taskId: string): Promise<TaskQueueEntry> {
  return invokeCommand("retry_task", { task_id: taskId }, TaskQueueEntrySchema);
}

/** List durable tasks, newest first. */
export function listTasks(limit = 50): Promise<TaskQueueEntry[]> {
  return invokeCommand("list_tasks", { limit }, z.array(TaskQueueEntrySchema));
}

/** Fetch one durable task by id. */
export function getTaskState(taskId: string): Promise<TaskQueueEntry> {
  return invokeCommand(
    "get_task_state",
    { task_id: taskId },
    TaskQueueEntrySchema,
  );
}

/** Tool framework (re-exported contracts schemas for the UI). */
export {
  ToolIdSchema,
  ToolInvocationSchema,
  ToolManifestSchema,
  ToolReceiptSchema,
};
export type { ToolId, ToolInvocation, ToolManifest, ToolReceipt };

/** List registered tool manifests. */
export function listTools(): Promise<ToolManifest[]> {
  return invokeCommand("list_tools", undefined, z.array(ToolManifestSchema));
}

/** Describe one tool manifest. */
export function describeTool(toolId: string): Promise<ToolManifest> {
  return invokeCommand(
    "describe_tool",
    { tool_id: toolId },
    ToolManifestSchema,
  );
}

function toolArgs(
  toolId: string,
  taskId: string,
  params: unknown,
  agentId?: string,
): Record<string, unknown> {
  return {
    tool_id: toolId,
    task_id: taskId,
    agent_id: agentId ?? "agent-local",
    params: params ?? {},
  };
}

/** Dry-run a tool: validate + policy-evaluate, no side effects. */
export function invokeToolDryRun(
  toolId: string,
  taskId: string,
  params: unknown,
  agentId?: string,
): Promise<ToolReceipt> {
  return invokeCommand(
    "invoke_tool_dry_run",
    toolArgs(toolId, taskId, params, agentId),
    ToolReceiptSchema,
  );
}

/** Live-invoke a tool: policy-gated, audited, compensated when risky. */
export function invokeTool(
  toolId: string,
  taskId: string,
  params: unknown,
  agentId?: string,
): Promise<ToolReceipt> {
  return invokeCommand(
    "invoke_tool",
    toolArgs(toolId, taskId, params, agentId),
    ToolReceiptSchema,
  );
}

/** Computer-use safety envelope status (always available, read-only). */
export const ComputerSafetyStatusSchema = z
  .object({
    enabled: z.boolean(),
    dryRunDefault: z.boolean(),
    emergencyStopped: z.boolean(),
    actionsThisMinute: z.number().int().nonnegative(),
    maxActionsPerMinute: z.number().int().positive(),
    viewportWidth: z.number().int().positive(),
    viewportHeight: z.number().int().positive(),
    minConfidence: z.number().min(0).max(1),
  })
  .strict();
export type ComputerSafetyStatus = z.infer<typeof ComputerSafetyStatusSchema>;

/** Fetch the computer-use safety envelope status. */
export function getComputerSafetyStatus(): Promise<ComputerSafetyStatus> {
  return invokeCommand(
    "computer_safety_status",
    undefined,
    ComputerSafetyStatusSchema,
  );
}

/** Latch the emergency stop (cancels active tasks, blocks new actions). */
export function computerEmergencyStop(): Promise<ComputerSafetyStatus> {
  return invokeCommand(
    "computer_emergency_stop",
    undefined,
    ComputerSafetyStatusSchema,
  );
}

/** Clear the emergency stop latch (explicit reset; does not resume tasks). */
export function computerResetStop(): Promise<ComputerSafetyStatus> {
  return invokeCommand(
    "computer_reset_stop",
    undefined,
    ComputerSafetyStatusSchema,
  );
}

/** Context budgeting (re-exported contracts schemas for the UI). */
export { ContextBudgetStatusSchema, ContextCompactionReceiptSchema };
export type { ContextBudgetStatus, ContextCompactionReceipt };

/** Fetch the deterministic per-layer budget for one task (read-only). */
export function getContextBudgetStatus(
  taskId: string,
  totalBudget?: number,
): Promise<ContextBudgetStatus> {
  return invokeCommand(
    "context_budget_status",
    { task_id: taskId, total_budget: totalBudget ?? null },
    ContextBudgetStatusSchema,
  );
}

/** TypeCompact one task's context and persist the receipt as evidence. */
export function compactTaskContext(
  taskId: string,
  totalBudget?: number,
): Promise<ContextCompactionReceipt> {
  return invokeCommand(
    "context_compact",
    { task_id: taskId, total_budget: totalBudget ?? null },
    ContextCompactionReceiptSchema,
  );
}

/** Run the diagnostic loop and TypeCompact its context snapshot. */
export function compactDiagnosticContext(
  totalBudget?: number,
): Promise<ContextCompactionReceipt> {
  return invokeCommand(
    "context_diagnostic_snapshot",
    { total_budget: totalBudget ?? null },
    ContextCompactionReceiptSchema,
  );
}

/** Tool-RAG routing (re-exported contracts schemas for the UI). */
export { ToolCandidateSchema, ToolRagQuerySchema };
export type { ToolCandidate, ToolRagQuery };

/** Rank registered tools for an intent (read-only, deterministic). */
export function queryToolCandidates(
  query: ToolRagQuery,
): Promise<ToolCandidate[]> {
  const parsed = ToolRagQuerySchema.safeParse(query);
  if (!parsed.success) {
    return Promise.reject(
      new Error(`Invalid tool query: ${parsed.error.message}`),
    );
  }
  return invokeCommand(
    "query_tool_candidates",
    {
      intent: parsed.data.intent,
      capabilities: parsed.data.capabilities ?? [],
      risk_ceiling: parsed.data.riskCeiling ?? null,
      require_evidence: parsed.data.requireEvidence ?? false,
      top_k: parsed.data.topK ?? null,
    },
    z.array(ToolCandidateSchema),
  );
}

export type { HeartbeatEvent };
export { HeartbeatEventSchema };
