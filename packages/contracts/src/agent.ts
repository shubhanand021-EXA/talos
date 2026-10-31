import { z } from "zod";
import {
  AgentIdSchema,
  IsoDateTimeSchema,
  SessionIdSchema,
  TaskIdSchema,
} from "./identity.js";

export const AgentRoleSchema = z.enum([
  "orchestrator",
  "coder",
  "browser",
  "computer-use",
  "auditor",
  "researcher",
  "custom",
]);
export type AgentRole = z.infer<typeof AgentRoleSchema>;

export const AgentStatusSchema = z.enum([
  "idle",
  "starting",
  "running",
  "waiting_policy",
  "failed",
  "terminated",
  "completed",
]);
export type AgentStatus = z.infer<typeof AgentStatusSchema>;

export const AgentDescriptorSchema = z
  .object({
    id: AgentIdSchema,
    displayName: z.string().min(1).max(128),
    role: AgentRoleSchema,
    status: AgentStatusSchema,
    sessionId: SessionIdSchema,
    taskId: TaskIdSchema.optional(),
    capabilities: z.array(z.string().min(1).max(64)).max(64),
    createdAt: IsoDateTimeSchema,
    updatedAt: IsoDateTimeSchema,
    metadata: z.record(z.string(), z.unknown()).optional(),
  })
  .strict();
export type AgentDescriptor = z.infer<typeof AgentDescriptorSchema>;
