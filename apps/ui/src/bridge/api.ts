import { z } from "zod";
import {
  ActionRequestSchema,
  ApprovalRequestSchema,
  AssembledContextMetadataSchema,
  AssembledLayerSchema,
  AssemblyAllocationSchema,
  AssemblyCompactionSchema,
  CompensationExecutionSchema,
  CompensationStepViewSchema,
  ContextAssemblyConfigSchema,
  ContextBudgetStatusSchema,
  ContextCompactionReceiptSchema,
  DelegateTaskInputSchema,
  DelegationResultSchema,
  HeartbeatEventSchema,
  ModelDescriptorSchema,
  PlanDocumentSchema,
  PolicyEvaluationSchema,
  ProviderIdSchema,
  ProviderReceiptSchema,
  ProviderStatusSchema,
  SpecialistRoleSchema,
  SubmitTaskInputSchema,
  SystemStatusSchema,
  TaskModeSchema,
  TaskQueueEntrySchema,
  TaskTreeNodeSchema,
  TokenUsageSchema,
  ToolCandidateSchema,
  ToolIdSchema,
  ToolInvocationSchema,
  ToolManifestSchema,
  ToolReceiptSchema,
  ToolRagQuerySchema,
  UsageRecordSchema,
  UsageSummarySchema,
  WorkspaceInfoSchema,
  IndexStatusSchema,
  MemoryQuerySchema,
  MemorySearchResultSchema,
  SkillSchema,
  type ActionRequest,
  type AssembledContextMetadata,
  type AssembledLayer,
  type AssemblyAllocation,
  type AssemblyCompaction,
  type ContextAssemblyConfig,
  type ContextCompactionReceipt,
  type ContextBudgetStatus,
  type HeartbeatEvent,
  type ModelDescriptor,
  type PolicyEvaluation,
  type ProviderId,
  type ProviderReceipt,
  type ProviderStatus,
  type SubmitTaskInput,
  type SystemStatus,
  type TaskQueueEntry,
  type TaskTreeNode,
  type TokenUsage,
  type ToolCandidate,
  type ToolId,
  type ToolInvocation,
  type ToolManifest,
  type ToolReceipt,
  type ToolRagQuery,
  type UsageRecord,
  type UsageSummary,
  type WorkspaceInfo,
  type IndexStatus,
  type MemoryQuery,
  type MemorySearchResult,
  type Skill,
  type ApprovalRequest as ApprovalRequestType,
  type CompensationExecution as CompensationExecutionType,
  type CompensationStepView as CompensationStepViewType,
  type PlanDocument as PlanDocumentType,
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

/** Delegation (re-exported contracts schemas for the UI). */
export {
  DelegateTaskInputSchema,
  DelegationResultSchema,
  SpecialistRoleSchema,
  TaskTreeNodeSchema,
};
export type { TaskTreeNode };
export type { DelegationResult } from "@talos/contracts";

/** Delegate a parent task to scoped specialists (Supervisor fan-out). */
export function delegateTask(input: {
  parentTaskId: string;
  goal?: string;
  role?: "coder" | "browser" | "researcher";
  scope?: string;
}): Promise<{
  parent: TaskQueueEntry;
  children: TaskQueueEntry[];
  results: import("@talos/contracts").DelegationResult[];
  mergedStatus: string;
}> {
  const parsed = DelegateTaskInputSchema.safeParse({
    parentTaskId: input.parentTaskId,
    ...(input.goal !== undefined ? { goal: input.goal } : {}),
    ...(input.role !== undefined ? { role: input.role } : {}),
    ...(input.scope !== undefined ? { scope: input.scope } : {}),
  });
  if (!parsed.success) {
    return Promise.reject(
      new Error(`Invalid delegate input: ${parsed.error.message}`),
    );
  }
  return invokeCommand(
    "delegate_task",
    {
      parent_task_id: parsed.data.parentTaskId,
      goal: parsed.data.goal ?? null,
      role: parsed.data.role ?? null,
      scope: parsed.data.scope ?? null,
      deadline_ms: null,
      context_budget: null,
    },
    z.object({
      parent: TaskQueueEntrySchema,
      children: z.array(TaskQueueEntrySchema),
      results: z.array(DelegationResultSchema),
      mergedStatus: z.string(),
    }),
  );
}

/** Fetch the parent-child task tree rooted at `taskId`. */
export function getTaskTree(taskId: string): Promise<TaskTreeNode> {
  return invokeCommand(
    "get_task_tree",
    { task_id: taskId },
    TaskTreeNodeSchema,
  );
}

/** Cancel a parent and all its descendants. */
export function cancelSubtree(taskId: string): Promise<TaskQueueEntry> {
  return invokeCommand(
    "cancel_subtree",
    { task_id: taskId },
    TaskQueueEntrySchema,
  );
}

/** Workspace roots (re-exported contracts schemas for the UI). */
export { WorkspaceInfoSchema };
export type { WorkspaceInfo };

/** Isolate a workspace root for `taskId` (idempotent). */
export function createWorkspace(
  taskId: string,
  repoPath?: string,
): Promise<WorkspaceInfo> {
  if (taskId.trim() === "") {
    return Promise.reject(new Error("taskId is required"));
  }
  return invokeCommand(
    "create_workspace",
    {
      task_id: taskId,
      repo_path: repoPath ?? null,
    },
    WorkspaceInfoSchema,
  );
}

/** Fetch workspace info for `taskId` (rejects when none exists). */
export function getWorkspace(taskId: string): Promise<WorkspaceInfo> {
  return invokeCommand(
    "get_workspace",
    { task_id: taskId },
    WorkspaceInfoSchema,
  );
}

/** List all task workspaces, sorted by task id. */
export function listWorkspaces(): Promise<WorkspaceInfo[]> {
  return invokeCommand(
    "list_workspaces",
    undefined,
    z.array(WorkspaceInfoSchema),
  );
}

/** Remove a task workspace (idempotent, returns true when removed). */
export function cleanupWorkspace(taskId: string): Promise<boolean> {
  return invokeCommand("cleanup_workspace", { task_id: taskId }, z.boolean());
}

/** File-tree of entries under a workspace root (bounded depth). */
export const WorkspaceTreeSchema = z
  .object({
    path: z.string(),
    entries: z.array(z.unknown()),
    truncated: z.boolean(),
  })
  .strict();
export type WorkspaceTree = z.infer<typeof WorkspaceTreeSchema>;

/** List the workspace file tree for the Code panel. */
export function getWorkspaceTree(
  taskId: string,
  depth = 2,
): Promise<WorkspaceTree> {
  return invokeCommand(
    "get_workspace_tree",
    { task_id: taskId, depth },
    WorkspaceTreeSchema,
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
  mode?: "lexical" | "semantic" | "hybrid",
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
      mode: mode ?? null,
    },
    z.array(ToolCandidateSchema),
  );
}

/** Semantic memory (re-exported contracts schemas for the UI). */
export {
  IndexStatusSchema,
  MemoryQuerySchema,
  MemorySearchResultSchema,
  SkillSchema,
};
export type { IndexStatus, MemoryQuery, MemorySearchResult, Skill };

/** Hybrid memory search over claims, evidence, tasks, and skills. */
export function searchMemory(
  query: MemoryQuery,
): Promise<MemorySearchResult[]> {
  const parsed = MemoryQuerySchema.safeParse(query);
  if (!parsed.success) {
    return Promise.reject(
      new Error(`Invalid memory query: ${parsed.error.message}`),
    );
  }
  return invokeCommand(
    "search_memory",
    {
      query: parsed.data.query,
      top_k: parsed.data.topK ?? null,
      source_types: parsed.data.sourceTypes ?? null,
      task_id: parsed.data.taskId ?? null,
      verified_only: parsed.data.verifiedOnly ?? null,
      mode: parsed.data.mode ?? null,
    },
    z.array(MemorySearchResultSchema),
  );
}

/** Index status: entry counts, embedding backend, model probe. */
export function getIndexStatus(): Promise<IndexStatus> {
  return invokeCommand("get_index_status", undefined, IndexStatusSchema);
}

/** Full memory reindex (clears vectors, re-embeds from the ledger). */
export function rebuildIndex(): Promise<IndexStatus> {
  return invokeCommand("rebuild_index", undefined, IndexStatusSchema);
}

/** Skill registry (optional status filter). */
export function listSkills(status?: string): Promise<Skill[]> {
  return invokeCommand(
    "list_skills",
    { status: status ?? null },
    z.array(SkillSchema),
  );
}

/** LLM gateway (re-exported contracts schemas for the UI). */
export {
  ModelDescriptorSchema,
  ProviderIdSchema,
  ProviderReceiptSchema,
  ProviderStatusSchema,
  TokenUsageSchema,
  UsageRecordSchema,
  UsageSummarySchema,
};
export type {
  ModelDescriptor,
  ProviderId,
  ProviderReceipt,
  ProviderStatus,
  TokenUsage,
  UsageRecord,
  UsageSummary,
};

/** List providers with key presence + catalog models (no key values). */
export function listProviders(): Promise<ProviderStatus[]> {
  return invokeCommand(
    "list_providers",
    undefined,
    z.array(ProviderStatusSchema),
  );
}

/** List catalog descriptors for one provider. */
export function listModels(providerId: string): Promise<ModelDescriptor[]> {
  return invokeCommand(
    "list_models",
    { provider_id: providerId },
    z.array(ModelDescriptorSchema),
  );
}

/** Probe result: success/latency/message, never a key. */
export const ConnectionTestSchema = z
  .object({
    providerId: z.string().min(1).max(64),
    ok: z.boolean(),
    latencyMs: z.number().int().nonnegative().nullish(),
    message: z.string().min(1).max(2048),
  })
  .strict();
export type ConnectionTest = z.infer<typeof ConnectionTestSchema>;

/** Minimal gated probe completion against one provider. */
export function testProviderConnection(
  providerId: string,
  model?: string,
): Promise<ConnectionTest> {
  return invokeCommand(
    "test_provider_connection",
    { provider_id: providerId, model: model ?? null },
    ConnectionTestSchema,
  );
}

/** Cumulative token/cost session summary from the usage ledger. */
export function getUsageSummary(): Promise<UsageSummary> {
  return invokeCommand("get_usage_summary", undefined, UsageSummarySchema);
}

/** Store a provider key in the OS keychain. The value is never echoed. */
export function storeProviderKey(
  providerId: string,
  apiKey: string,
): Promise<boolean> {
  if (providerId.trim() === "" || apiKey === "") {
    return Promise.reject(new Error("providerId and apiKey are required"));
  }
  return invokeCommand(
    "store_provider_key",
    { provider_id: providerId, api_key: apiKey },
    z.boolean(),
  );
}

/** Remove a provider key from the OS keychain (idempotent). */
export function deleteProviderKey(providerId: string): Promise<boolean> {
  return invokeCommand(
    "delete_provider_key",
    { provider_id: providerId },
    z.boolean(),
  );
}

/** Context assembly (re-exported contracts schemas for the UI). */
export {
  AssembledContextMetadataSchema,
  AssembledLayerSchema,
  AssemblyAllocationSchema,
  AssemblyCompactionSchema,
  ContextAssemblyConfigSchema,
};
export type {
  AssembledContextMetadata,
  AssembledLayer,
  AssemblyAllocation,
  AssemblyCompaction,
  ContextAssemblyConfig,
};

/**
 * Assemble the budgeted prompt for a task and return metadata only
 * (structure, counts, hashes — never prompt text).
 */
export function previewContext(
  taskId: string,
  options?: {
    model?: string;
    intent?: string;
    instruction?: string;
    contextWindow?: number;
  },
): Promise<AssembledContextMetadata> {
  if (taskId.trim() === "") {
    return Promise.reject(new Error("taskId is required"));
  }
  return invokeCommand(
    "preview_context",
    {
      task_id: taskId,
      model: options?.model ?? null,
      intent: options?.intent ?? null,
      instruction: options?.instruction ?? null,
      context_window: options?.contextWindow ?? null,
    },
    AssembledContextMetadataSchema,
  );
}

/** Fetch the current assembly allocation settings. */
export function getContextConfig(): Promise<ContextAssemblyConfig> {
  return invokeCommand(
    "get_context_config",
    undefined,
    ContextAssemblyConfigSchema,
  );
}

/** Update allocation percentages (must sum to 100). */
export function updateContextConfig(
  allocation: AssemblyAllocation,
  reservedOutputPct: number,
): Promise<ContextAssemblyConfig> {
  const parsed = AssemblyAllocationSchema.safeParse(allocation);
  if (!parsed.success) {
    return Promise.reject(
      new Error(`Invalid allocation: ${parsed.error.message}`),
    );
  }
  return invokeCommand(
    "update_context_config",
    { allocation: parsed.data, reserved_output_pct: reservedOutputPct },
    ContextAssemblyConfigSchema,
  );
}

/** HITL approvals (re-exported contracts schemas for the UI). */
export {
  ApprovalRequestSchema,
  CompensationExecutionSchema,
  CompensationStepViewSchema,
  PlanDocumentSchema,
  TaskModeSchema,
};
export type {
  ApprovalRequest,
  CompensationExecution,
  CompensationStepView,
  PlanDocument,
  TaskMode,
} from "@talos/contracts";

/** List all waiting approvals (sweeps expiries first). */
export function getPendingApprovals(): Promise<ApprovalRequestType[]> {
  return invokeCommand(
    "get_pending_approvals",
    undefined,
    z.array(ApprovalRequestSchema),
  );
}

/** Grant a pending approval; the task resumes to `executing`. */
export function grantApproval(
  approvalId: string,
  dontAskAgain = false,
): Promise<ApprovalRequestType> {
  return invokeCommand(
    "grant_approval",
    { approval_id: approvalId, dont_ask_again: dontAskAgain },
    ApprovalRequestSchema,
  );
}

/** Deny a pending approval; the task moves to `failed`. */
export function denyApproval(
  approvalId: string,
  reason?: string,
): Promise<ApprovalRequestType> {
  return invokeCommand(
    "deny_approval",
    { approval_id: approvalId, reason: reason ?? null },
    ApprovalRequestSchema,
  );
}

/** Patch parameters; the engine re-evaluates the modified action. */
export function modifyApproval(
  approvalId: string,
  modifiedParams: unknown,
): Promise<ApprovalRequestType> {
  return invokeCommand(
    "modify_approval",
    { approval_id: approvalId, modified_params: modifiedParams ?? {} },
    ApprovalRequestSchema,
  );
}

/** Batch-approve low-risk reversibles for one task (high-risk untouched). */
export function batchApproveLowRisk(
  taskId: string,
): Promise<ApprovalRequestType[]> {
  return invokeCommand(
    "batch_approve_low_risk",
    { task_id: taskId },
    z.array(ApprovalRequestSchema),
  );
}

/** Switch the Plan/Act/Auto mode for one task. */
export function setTaskMode(
  taskId: string,
  mode: "plan" | "act" | "auto",
): Promise<string> {
  return invokeCommand("set_task_mode", { task_id: taskId, mode }, z.string());
}

/** Read the current mode for one task (defaults to `plan`). */
export function getTaskMode(taskId: string): Promise<string> {
  return invokeCommand("get_task_mode", { task_id: taskId }, z.string());
}

/** Fetch the ordered review list for a task (Plan mode). */
export function getPlanDocument(taskId: string): Promise<PlanDocumentType> {
  return invokeCommand(
    "get_plan_document",
    { task_id: taskId },
    PlanDocumentSchema,
  );
}

/** User-confirmed rollback: run compensation steps in order. */
export function executeCompensation(
  planId: string,
  taskId: string,
  failureReason?: string,
  steps: CompensationStepViewType[] = [],
): Promise<CompensationExecutionType> {
  return invokeCommand(
    "execute_compensation",
    {
      plan_id: planId,
      task_id: taskId,
      failure_reason: failureReason ?? null,
      steps,
    },
    CompensationExecutionSchema,
  );
}

export type { HeartbeatEvent };
export { HeartbeatEventSchema };
