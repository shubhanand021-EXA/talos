import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { z } from "zod";
import {
  AgentDescriptorSchema,
  ApprovalRequestSchema,
  BackupCreatedPayloadSchema,
  BackupRestoredPayloadSchema,
  EvidenceRecordSchema,
  HeartbeatEventSchema,
  IntegrityCheckCompletedPayloadSchema,
  IpcEventName,
  PolicyEvaluationSchema,
  type AgentDescriptor,
  type BackupCreatedPayload,
  type BackupRestoredPayload,
  type EvidenceRecord,
  type HeartbeatEvent,
  type IntegrityCheckCompletedPayload,
  type PolicyEvaluation,
} from "@talos/contracts";
import { isTauri } from "./client.js";
import { BridgeError } from "./errors.js";

/** Task payload carried by `taskCreated` / `taskUpdated`. */
export const DiagnosticTaskPayloadSchema = z
  .object({
    task: z
      .object({
        id: z.string().min(1).max(128),
        title: z.string().min(1).max(256),
        status: z.string().min(1).max(64),
      })
      .strict(),
  })
  .strict();
export type DiagnosticTaskPayload = z.infer<typeof DiagnosticTaskPayloadSchema>;

/** Queue task payload emitted by submit/cancel/retry (superset of diagnostic). */
export const QueueTaskPayloadSchema = z
  .object({
    task: z
      .object({
        id: z.string().min(1).max(128),
        title: z.string().min(1).max(256),
        status: z.string().min(1).max(64),
        priority: z.string().min(1).max(64).optional(),
        attempts: z.number().int().nonnegative().optional(),
        maxAttempts: z.number().int().positive().optional(),
      })
      .strict(),
  })
  .strict();
export type QueueTaskPayload = z.infer<typeof QueueTaskPayloadSchema>;

/** Payload of `agentSpawned`: a full contracts-validated agent descriptor. */
export const AgentSpawnedPayloadSchema = z
  .object({ agent: AgentDescriptorSchema })
  .strict();
export type AgentSpawnedPayload = z.infer<typeof AgentSpawnedPayloadSchema>;

/** Payload of `policyEvaluated`. */
export const PolicyEvaluatedPayloadSchema = z
  .object({ evaluation: PolicyEvaluationSchema })
  .strict();
export type PolicyEvaluatedPayload = z.infer<
  typeof PolicyEvaluatedPayloadSchema
>;

/** Payload of `evidenceUpdated`. */
export const EvidenceUpdatedPayloadSchema = z
  .object({ evidence: EvidenceRecordSchema })
  .strict();
export type EvidenceUpdatedPayload = z.infer<
  typeof EvidenceUpdatedPayloadSchema
>;

/** Payload of `computerSafetyChanged`: emergency-stop latch + cancel count. */
export const ComputerSafetyChangedPayloadSchema = z
  .object({
    emergencyStopped: z.boolean(),
    canceledTasks: z.number().int().nonnegative(),
    checkedAt: z.string().min(1),
  })
  .strict();
export type ComputerSafetyChangedPayload = z.infer<
  typeof ComputerSafetyChangedPayloadSchema
>;

/** Payload of `contextCompacted`: compaction receipt linkage + token delta. */
export const ContextCompactedPayloadSchema = z
  .object({
    taskId: z.string().min(1).max(128),
    evidenceId: z.string().min(1).max(128).nullish(),
    proofHash: z.string().min(1).max(256),
    originalTokens: z.number().int().nonnegative(),
    compactedTokens: z.number().int().nonnegative(),
    checkedAt: z.string().min(1),
  })
  .strict();
export type ContextCompactedPayload = z.infer<
  typeof ContextCompactedPayloadSchema
>;

/** Payload of `llmRequestStarted`: linkage + model, never prompts. */
export const LlmRequestStartedPayloadSchema = z
  .object({
    taskId: z.string().min(1).max(128),
    agentId: z.string().min(1).max(128),
    providerId: z.string().min(1).max(64),
    model: z.string().min(1).max(128),
    checkedAt: z.string().min(1),
  })
  .strict();
export type LlmRequestStartedPayload = z.infer<
  typeof LlmRequestStartedPayloadSchema
>;

/** Payload of `llmRequestCompleted`: cost metadata only, never content. */
export const LlmRequestCompletedPayloadSchema = z
  .object({
    taskId: z.string().min(1).max(128),
    providerId: z.string().min(1).max(64),
    model: z.string().min(1).max(128).nullish(),
    totalTokens: z.number().int().nonnegative().nullish(),
    estimatedCostUsd: z.number().min(0).nullish(),
    latencyMs: z.number().int().nonnegative().nullish(),
    ok: z.boolean().nullish(),
    checkedAt: z.string().min(1),
  })
  .strict();
export type LlmRequestCompletedPayload = z.infer<
  typeof LlmRequestCompletedPayloadSchema
>;

/** Payload of `llmBudgetWarning`: spend crossing 80% of a task cap. */
export const LlmBudgetWarningPayloadSchema = z
  .object({
    taskId: z.string().min(1).max(128),
    spentUsd: z.number().min(0),
    budgetUsd: z.number().min(0),
    checkedAt: z.string().min(1),
  })
  .strict();
export type LlmBudgetWarningPayload = z.infer<
  typeof LlmBudgetWarningPayloadSchema
>;

/** Payload of `browserWorkerCrashed`: worker failure linkage. */
export const BrowserWorkerCrashedPayloadSchema = z
  .object({
    taskId: z.string().min(1).max(128),
    toolId: z.string().min(1).max(64),
    code: z.string().min(1).max(64),
    checkedAt: z.string().min(1),
  })
  .strict();
export type BrowserWorkerCrashedPayload = z.infer<
  typeof BrowserWorkerCrashedPayloadSchema
>;

/** Payload of `agentDelegated` / `childTaskSpawned`. */
export const DelegationSpawnPayloadSchema = z
  .object({
    parentTaskId: z.string().min(1).max(128),
    childTaskId: z.string().min(1).max(128),
    role: z.string().min(1).max(32),
  })
  .strict();
export type DelegationSpawnPayload = z.infer<
  typeof DelegationSpawnPayloadSchema
>;

/** Payload of `childTaskCompleted`. */
export const ChildTaskCompletedPayloadSchema = z
  .object({
    parentTaskId: z.string().min(1).max(128),
    childTaskId: z.string().min(1).max(128),
    status: z.string().min(1).max(32),
  })
  .strict();
export type ChildTaskCompletedPayload = z.infer<
  typeof ChildTaskCompletedPayloadSchema
>;

/** Payload of `childTaskFailed`. */
export const ChildTaskFailedPayloadSchema = z
  .object({
    parentTaskId: z.string().min(1).max(128),
    childTaskId: z.string().min(1).max(128),
    error: z.string().min(1).max(2048),
  })
  .strict();
export type ChildTaskFailedPayload = z.infer<
  typeof ChildTaskFailedPayloadSchema
>;

/** Payload of `resultsMerged`. */
export const ResultsMergedPayloadSchema = z
  .object({
    parentTaskId: z.string().min(1).max(128),
    status: z.string().min(1).max(32),
    artifactCount: z.number().int().nonnegative(),
  })
  .strict();
export type ResultsMergedPayload = z.infer<typeof ResultsMergedPayloadSchema>;

/** Payload of `childBudgetExceeded` (amounts only, never prompts or keys). */
export const ChildBudgetExceededPayloadSchema = z
  .object({
    childTaskId: z.string().min(1).max(128),
    reservedUsd: z.number().min(0),
    spentUsd: z.number().min(0),
    checkedAt: z.string().min(1),
  })
  .strict();
export type ChildBudgetExceededPayload = z.infer<
  typeof ChildBudgetExceededPayloadSchema
>;

/** Payload of `workspaceUpdated`. */
export const WorkspaceUpdatedPayloadSchema = z
  .object({
    taskId: z.string().min(1).max(128),
    path: z.string().min(1).max(1024),
    status: z.string().min(1).max(32),
    checkedAt: z.string().min(1),
  })
  .strict();
export type WorkspaceUpdatedPayload = z.infer<
  typeof WorkspaceUpdatedPayloadSchema
>;

/** Payload of `memoryIndexed` (counts only, never query text). */
export const MemoryIndexedPayloadSchema = z
  .object({
    queryChars: z.number().int().nonnegative(),
    resultCount: z.number().int().nonnegative(),
    mode: z.string().min(1).max(32),
    checkedAt: z.string().min(1),
  })
  .strict();
export type MemoryIndexedPayload = z.infer<typeof MemoryIndexedPayloadSchema>;

/** Payload of `skillCreated`. */
export const SkillCreatedPayloadSchema = z
  .object({
    skillId: z.string().min(1).max(128),
    status: z.string().min(1).max(32),
    checkedAt: z.string().min(1),
  })
  .strict();
export type SkillCreatedPayload = z.infer<typeof SkillCreatedPayloadSchema>;

/** Payload of `indexRebuilt`. */
export const IndexRebuiltPayloadSchema = z
  .object({
    entriesAdded: z.number().int().nonnegative(),
    checkedAt: z.string().min(1),
  })
  .strict();
export type IndexRebuiltPayload = z.infer<typeof IndexRebuiltPayloadSchema>;

/** Payload of `approvalRequested`: the full ApprovalSheet record. */
export const ApprovalRequestedPayloadSchema = z
  .object({
    approvalId: z.string().min(1).max(128),
    taskId: z.string().min(1).max(128),
    approval: ApprovalRequestSchema,
  })
  .strict();
export type ApprovalRequestedPayload = z.infer<
  typeof ApprovalRequestedPayloadSchema
>;

/** Payload of `approvalGranted`. */
export const ApprovalGrantedPayloadSchema = z
  .object({
    approvalId: z.string().min(1).max(128),
    taskId: z.string().min(1).max(128),
    grantedBy: z.string().min(1).max(128),
    grantedAt: z.string().min(1),
  })
  .strict();
export type ApprovalGrantedPayload = z.infer<
  typeof ApprovalGrantedPayloadSchema
>;

/** Payload of `approvalDenied`. */
export const ApprovalDeniedPayloadSchema = z
  .object({
    approvalId: z.string().min(1).max(128),
    taskId: z.string().min(1).max(128),
    deniedBy: z.string().min(1).max(128),
    reason: z.string().min(1).max(2048),
  })
  .strict();
export type ApprovalDeniedPayload = z.infer<typeof ApprovalDeniedPayloadSchema>;

/** Payload of `approvalExpired`. */
export const ApprovalExpiredPayloadSchema = z
  .object({
    approvalId: z.string().min(1).max(128),
    taskId: z.string().min(1).max(128),
  })
  .strict();
export type ApprovalExpiredPayload = z.infer<
  typeof ApprovalExpiredPayloadSchema
>;

/** Payload of `compensationExecuted`. */
export const CompensationExecutedPayloadSchema = z
  .object({
    planId: z.string().min(1).max(128),
    taskId: z.string().min(1).max(128),
    stepsCompleted: z.number().int().nonnegative(),
    status: z.string().min(1).max(64),
  })
  .strict();
export type CompensationExecutedPayload = z.infer<
  typeof CompensationExecutedPayloadSchema
>;

/** Payload of `actionVerificationFailed`: frame did not change as targeted. */
export const ActionVerificationFailedPayloadSchema = z
  .object({
    taskId: z.string().min(1).max(128),
    toolId: z.string().min(1).max(64),
    beforeHash: z.unknown().nullish(),
    afterHash: z.unknown().nullish(),
    detail: z.unknown().nullish(),
    checkedAt: z.string().min(1),
  })
  .strict();
export type ActionVerificationFailedPayload = z.infer<
  typeof ActionVerificationFailedPayloadSchema
>;

/** Payload of `computerFocusLost`: focus escaped mid-action. */
export const ComputerFocusLostPayloadSchema = z
  .object({
    taskId: z.string().min(1).max(128),
    toolId: z.string().min(1).max(64),
    checkedAt: z.string().min(1),
  })
  .strict();
export type ComputerFocusLostPayload = z.infer<
  typeof ComputerFocusLostPayloadSchema
>;

/** Subscribe to one typed Tauri event. Resolves to an unlisten function. */
export async function subscribeTalosEvent<T>(
  event: string,
  handler: (payload: T) => void,
  schema?: z.ZodType<T>,
): Promise<UnlistenFn> {
  if (!isTauri()) {
    throw new BridgeError(
      "backend_unavailable",
      `Cannot subscribe to "${event}" outside the Tauri container`,
    );
  }
  try {
    return await listen<unknown>(event, (tauriEvent) => {
      if (schema === undefined) {
        handler(tauriEvent.payload as T);
        return;
      }
      const parsed = schema.safeParse(tauriEvent.payload);
      if (parsed.success) {
        handler(parsed.data);
      }
    });
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    throw new BridgeError(
      "subscription_failed",
      `Subscribe "${event}" failed: ${message}`,
    );
  }
}

export {
  HeartbeatEventSchema,
  IpcEventName,
  BackupCreatedPayloadSchema,
  BackupRestoredPayloadSchema,
  IntegrityCheckCompletedPayloadSchema,
};
export type {
  AgentDescriptor,
  EvidenceRecord,
  HeartbeatEvent,
  PolicyEvaluation,
  BackupCreatedPayload,
  BackupRestoredPayload,
  IntegrityCheckCompletedPayload,
};
