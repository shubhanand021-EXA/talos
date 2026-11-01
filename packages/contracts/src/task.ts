import { z } from "zod";
import {
  AgentIdSchema,
  IsoDateTimeSchema,
  SessionIdSchema,
  TaskIdSchema,
} from "./identity.js";
import { ExecutionFidelitySchema } from "./fidelity.js";

export const TaskStatusSchema = z.enum([
  "queued",
  "planning",
  "executing",
  "waiting_policy",
  "waiting_evidence",
  "waiting_compensation",
  "canceling",
  "canceled",
  "completed",
  "failed",
  "failed_needs_manual_review",
  "timed_out",
]);
export type TaskStatus = z.infer<typeof TaskStatusSchema>;

/** States from which no further transition is allowed. */
export const TERMINAL_TASK_STATUSES = [
  "completed",
  "failed",
  "failed_needs_manual_review",
  "canceled",
  "timed_out",
] as const;
export type TerminalTaskStatus = (typeof TERMINAL_TASK_STATUSES)[number];

export const TaskPrioritySchema = z.enum(["low", "normal", "high", "critical"]);
export type TaskPriority = z.infer<typeof TaskPrioritySchema>;

export const TaskRecordSchema = z
  .object({
    id: TaskIdSchema,
    sessionId: SessionIdSchema,
    title: z.string().min(1).max(256),
    description: z.string().max(4096).optional(),
    status: TaskStatusSchema,
    priority: TaskPrioritySchema,
    agentId: AgentIdSchema.optional(),
    parentTaskId: TaskIdSchema.optional(),
    executionFidelity: ExecutionFidelitySchema,
    createdAt: IsoDateTimeSchema,
    updatedAt: IsoDateTimeSchema,
    completedAt: IsoDateTimeSchema.optional(),
    metadata: z.record(z.string(), z.unknown()).optional(),
  })
  .strict();
export type TaskRecord = z.infer<typeof TaskRecordSchema>;

/** Input for `submit_task`: validated at the UI boundary and the IPC layer. */
export const SubmitTaskInputSchema = z
  .object({
    title: z.string().min(1).max(256),
    description: z.string().max(4096).optional(),
    priority: TaskPrioritySchema.optional(),
    timeoutMs: z.number().int().min(100).max(3_600_000).optional(),
    maxAttempts: z.number().int().min(1).max(10).optional(),
    scenario: z
      .enum([
        "quick_success",
        "delayed_success",
        "retryable_failure",
        "non_retryable_failure",
        "cancellation_sensitive",
      ])
      .optional(),
  })
  .strict();
export type SubmitTaskInput = z.infer<typeof SubmitTaskInputSchema>;

/** Durable queue entry surfaced by `list_tasks` / `list_queue` / `get_task_state`. */
export const TaskQueueEntrySchema = z
  .object({
    id: TaskIdSchema,
    title: z.string().min(1).max(256),
    status: TaskStatusSchema,
    priority: TaskPrioritySchema,
    attempts: z.number().int().nonnegative(),
    maxAttempts: z.number().int().min(1).max(10),
    timeoutMs: z.number().int().positive(),
    scenario: z.string().min(1).max(64),
    lastError: z.string().max(2048).nullish(),
    nextRetryAt: IsoDateTimeSchema.nullish(),
    parentTaskId: TaskIdSchema.nullish(),
    executionFidelity: ExecutionFidelitySchema,
    createdAt: IsoDateTimeSchema,
    updatedAt: IsoDateTimeSchema,
  })
  .strict();
export type TaskQueueEntry = z.infer<typeof TaskQueueEntrySchema>;

/** One node of a parent-child task tree (`get_task_tree` / `cancel_subtree`). */
export type TaskTreeNode = {
  task: TaskQueueEntry;
  children: TaskTreeNode[];
};
export const TaskTreeNodeSchema: z.ZodType<TaskTreeNode> = z
  .object({
    task: TaskQueueEntrySchema,
    children: z
      .array(z.lazy((): z.ZodType<TaskTreeNode> => TaskTreeNodeSchema))
      .max(100),
  })
  .strict();
