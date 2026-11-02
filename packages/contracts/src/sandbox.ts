import { z } from "zod";
import { TaskIdSchema } from "./identity.js";

/** OS sandbox profile for shell/process execution. */
export const SandboxProfileSchema = z.enum([
  "read_only",
  "workspace_write",
  "danger_full_access",
]);
export type SandboxProfile = z.infer<typeof SandboxProfileSchema>;

/** OS sandbox configuration (mirrors the Rust `sandbox::SandboxConfig`). */
export const SandboxConfigSchema = z
  .object({
    profile: SandboxProfileSchema,
    workspaceRoot: z.string().min(1).max(1024),
    networkAllowed: z.boolean(),
    allowedHosts: z.array(z.string().min(1).max(253)).max(16),
    cpuLimitPercent: z.number().int().min(1).max(100).nullish(),
    memoryLimitMb: z.number().int().min(16).max(65536).nullish(),
    timeoutMs: z.number().int().min(100).max(300_000),
    allowedCommands: z.array(z.string().min(1).max(64)).max(64),
  })
  .strict();
export type SandboxConfig = z.infer<typeof SandboxConfigSchema>;

/** Sandbox enforcement receipt (recorded in tool output + evidence). */
export const SandboxReceiptSchema = z
  .object({
    profile: SandboxProfileSchema,
    enforced: z.boolean(),
    enforcementMethod: z.string().min(1).max(256),
    networkPolicy: z.string().min(1).max(256),
    violations: z.array(z.string().max(1024)).max(16),
  })
  .strict();
export type SandboxReceipt = z.infer<typeof SandboxReceiptSchema>;

/** Platform sandbox backend status (`get_sandbox_status`). */
export const SandboxBackendSchema = z
  .object({
    name: z.string().min(1).max(64),
    available: z.boolean(),
    enforcementMethod: z.string().min(1).max(256),
    detail: z.string().max(1024),
  })
  .strict();
export type SandboxBackend = z.infer<typeof SandboxBackendSchema>;

export const SandboxStatusSchema = z
  .object({
    platform: z.string().min(1).max(64),
    failClosed: z.boolean(),
    backends: z.array(SandboxBackendSchema).max(8),
    checkedAt: z.string().min(1).max(64),
  })
  .strict();
export type SandboxStatus = z.infer<typeof SandboxStatusSchema>;

/** Sandbox profile resolution for one task (`get_sandbox_profile_for_task`). */
export const SandboxProfileForTaskSchema = z
  .object({
    taskId: TaskIdSchema,
    profile: SandboxProfileSchema,
    enforced: z.boolean(),
    enforcementMethod: z.string().min(1).max(256),
    reason: z.string().max(1024),
  })
  .strict();
export type SandboxProfileForTask = z.infer<typeof SandboxProfileForTaskSchema>;

/** `sandboxEnforced` event payload. */
export const SandboxEnforcedPayloadSchema = z
  .object({
    taskId: TaskIdSchema,
    toolId: z.string().min(1).max(64),
    profile: SandboxProfileSchema,
    enforcementMethod: z.string().min(1).max(256),
    checkedAt: z.string().min(1).max(64),
  })
  .strict();
export type SandboxEnforcedPayload = z.infer<
  typeof SandboxEnforcedPayloadSchema
>;

/** `sandboxViolation` event payload. */
export const SandboxViolationPayloadSchema = z
  .object({
    taskId: TaskIdSchema,
    toolId: z.string().min(1).max(64).optional(),
    profile: SandboxProfileSchema.optional(),
    code: z.string().min(1).max(64),
    detail: z.string().min(1).max(2048),
    checkedAt: z.string().min(1).max(64),
  })
  .strict();
export type SandboxViolationPayload = z.infer<
  typeof SandboxViolationPayloadSchema
>;
