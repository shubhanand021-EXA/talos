import { z } from "zod";

/**
 * BYOC remote runner contracts (Batch 5): shared read model for remote
 * execution targets (SSH/Docker). Secrets never cross this boundary —
 * `runners.json` holds key *paths*, never key material; the daemon UI only
 * ever sees ids, kinds, and status.
 */

/** Where a live tool executes (mirrors `tool_runtime::ExecutionTarget`). */
export const ExecutionTargetTypeSchema = z.enum(["local", "remote"]);
export type ExecutionTargetType = z.infer<typeof ExecutionTargetTypeSchema>;

/** Runner transport. */
export const RunnerKindSchema = z.enum(["ssh", "docker"]);
export type RunnerKind = z.infer<typeof RunnerKindSchema>;

/** One validated runner definition (secret-free; mirrors `runners.json`). */
export const RemoteRunnerConfigSchema = z
  .object({
    id: z
      .string()
      .min(1)
      .max(64)
      .regex(/^[a-z0-9-_]+$/, "id must be [a-z0-9-_]{1,64}"),
    kind: RunnerKindSchema,
    host: z.string().min(1).max(1024).optional(),
    user: z.string().min(1).max(256).optional(),
    keyPath: z.string().min(1).max(1024).optional(),
    port: z.number().int().min(1).max(65535).optional(),
    remoteDir: z.string().min(1).max(1024).optional(),
    dockerContext: z.string().min(1).max(256).optional(),
    dockerImage: z.string().min(1).max(512).optional(),
    workdir: z.string().min(1).max(256).optional(),
    cpus: z.string().min(1).max(16).optional(),
    memoryMb: z.number().int().min(128).max(1048576).optional(),
    networkMode: z.string().min(1).max(64).optional(),
    enabled: z.boolean().default(true),
  })
  .strict();
export type RemoteRunnerConfig = z.infer<typeof RemoteRunnerConfigSchema>;

/** One runner status row (never carries key material). */
export const RunnerStatusViewSchema = z
  .object({
    id: z.string().min(1).max(64),
    kind: RunnerKindSchema,
    enabled: z.boolean(),
    lastTestOk: z.boolean().nullish(),
    lastTestMessage: z.string().max(512).nullish(),
  })
  .strict();
export type RunnerStatusView = z.infer<typeof RunnerStatusViewSchema>;

/** Assign (or clear with `null`) a task's BYOC runner. */
export const SetTaskRunnerSchema = z
  .object({
    taskId: z.string().min(1).max(128),
    runnerId: z.string().min(1).max(64).nullish(),
  })
  .strict();
export type SetTaskRunner = z.infer<typeof SetTaskRunnerSchema>;
