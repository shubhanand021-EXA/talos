import { z } from "zod";
import {
  AgentIdSchema,
  IsoDateTimeSchema,
  TalosIdSchema,
  TaskIdSchema,
} from "./identity.js";

export const ToolIdSchema = z.enum([
  "fs.read",
  "fs.list",
  "fs.search",
  "fs.write",
  "process.exec",
]);
export type ToolId = z.infer<typeof ToolIdSchema>;

export const ToolStatusSchema = z.enum([
  "success",
  "failed",
  "denied",
  "disabled",
]);
export type ToolStatus = z.infer<typeof ToolStatusSchema>;

export const ToolErrorSchema = z
  .object({
    code: z.string().min(1).max(64),
    message: z.string().min(1).max(2048),
  })
  .strict();
export type ToolError = z.infer<typeof ToolErrorSchema>;

export const ToolManifestSchema = z
  .object({
    id: ToolIdSchema,
    name: z.string().min(1).max(128),
    version: z.string().min(1).max(32),
    description: z.string().min(1).max(2048),
    capabilities: z.array(z.string().min(1).max(64)).max(16),
    riskLevel: z.string().min(1).max(32),
    reversible: z.boolean(),
    requiresEvidence: z.boolean(),
    requiresCompensation: z.boolean(),
    enabledByDefault: z.boolean(),
    inputSchema: z.unknown(),
    outputSchema: z.unknown(),
  })
  .strict();
export type ToolManifest = z.infer<typeof ToolManifestSchema>;

export const ToolInvocationSchema = z
  .object({
    toolId: ToolIdSchema,
    taskId: TaskIdSchema,
    agentId: AgentIdSchema.optional(),
    params: z.unknown(),
    dryRun: z.boolean().optional(),
  })
  .strict();
export type ToolInvocation = z.infer<typeof ToolInvocationSchema>;

export const ToolReceiptSchema = z
  .object({
    id: TalosIdSchema,
    toolId: ToolIdSchema,
    taskId: TaskIdSchema,
    dryRun: z.boolean(),
    status: ToolStatusSchema,
    output: z.unknown(),
    error: ToolErrorSchema.optional(),
    policyDecision: z.string().min(1).max(64),
    policyReason: z.string().min(1).max(2048),
    evidenceId: z.string().min(1).max(128).optional(),
    compensationPlanId: z.string().min(1).max(128).optional(),
    executedAt: IsoDateTimeSchema,
  })
  .strict();
export type ToolReceipt = z.infer<typeof ToolReceiptSchema>;
