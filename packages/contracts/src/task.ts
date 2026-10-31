import { z } from "zod";
import {
  AgentIdSchema,
  IsoDateTimeSchema,
  SessionIdSchema,
  TaskIdSchema,
} from "./identity.js";

export const TaskStatusSchema = z.enum([
  "queued",
  "planning",
  "executing",
  "blocked",
  "completed",
  "failed",
  "canceled",
]);
export type TaskStatus = z.infer<typeof TaskStatusSchema>;

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
    createdAt: IsoDateTimeSchema,
    updatedAt: IsoDateTimeSchema,
    completedAt: IsoDateTimeSchema.optional(),
    metadata: z.record(z.string(), z.unknown()).optional(),
  })
  .strict();
export type TaskRecord = z.infer<typeof TaskRecordSchema>;
