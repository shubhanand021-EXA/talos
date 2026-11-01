import { z } from "zod";
import { IsoDateTimeSchema, TaskIdSchema } from "./identity.js";

/** Isolated coding workspace for one task (worktree or temp dir). */
export const WorkspaceInfoSchema = z
  .object({
    taskId: TaskIdSchema,
    root: z.string().min(1).max(1024),
    branch: z.string().min(1).max(128),
    isGitWorktree: z.boolean(),
  })
  .strict();
export type WorkspaceInfo = z.infer<typeof WorkspaceInfoSchema>;

/** Bounded shell execution receipt (hashes, never raw secrets). */
export const ShellReceiptSchema = z
  .object({
    command: z.string().min(1).max(256),
    exitCode: z.number().int().nullish(),
    stdoutHash: z.string().min(1).max(256),
    stderrHash: z.string().min(1).max(256),
    truncated: z.boolean(),
    timedOut: z.boolean(),
    durationMs: z.number().int().nonnegative(),
    workspacePath: z.string().min(1).max(1024),
  })
  .strict();
export type ShellReceipt = z.infer<typeof ShellReceiptSchema>;

/** Structured test-runner result stored as evidence. */
export const TestResultSchema = z
  .object({
    framework: z.string().min(1).max(32),
    total: z.number().int().nonnegative(),
    passed: z.number().int().nonnegative(),
    failed: z.number().int().nonnegative(),
    errors: z.number().int().nonnegative(),
    outputHash: z.string().min(1).max(256),
    durationMs: z.number().int().nonnegative(),
    exitCode: z.number().int().nullish(),
  })
  .strict();
export type TestResult = z.infer<typeof TestResultSchema>;

/** File-tree node for the workspace panel. */
export const WorkspaceTreeNodeSchema = z
  .object({
    path: z.string().min(1).max(1024),
    entries: z.array(z.unknown()).max(500),
    truncated: z.boolean(),
  })
  .strict();
export type WorkspaceTreeNode = z.infer<typeof WorkspaceTreeNodeSchema>;

/** Payload of `workspaceUpdated`. */
export const WorkspaceUpdatedPayloadSchema = z
  .object({
    taskId: z.string().min(1).max(128),
    path: z.string().min(1).max(1024),
    status: z.string().min(1).max(32),
    checkedAt: IsoDateTimeSchema,
  })
  .strict();
export type WorkspaceUpdatedPayload = z.infer<
  typeof WorkspaceUpdatedPayloadSchema
>;
