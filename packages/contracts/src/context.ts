import { z } from "zod";
import { IsoDateTimeSchema, TalosIdSchema, TaskIdSchema } from "./identity.js";

export const ContextLayerSchema = z.enum([
  "system",
  "session",
  "task",
  "evidence",
  "tool",
  "scratchpad",
  "safety",
]);
export type ContextLayer = z.infer<typeof ContextLayerSchema>;

/** Per-layer budget accounting (mirrors Rust `LayerUsage`, camelCase). */
export const ContextLayerBudgetSchema = z
  .object({
    layer: ContextLayerSchema,
    usedTokens: z.number().int().nonnegative(),
    maxTokens: z.number().int().nonnegative(),
    overflow: z.boolean(),
    protected: z.boolean(),
    segmentCount: z.number().int().nonnegative(),
  })
  .strict();
export type ContextLayerBudget = z.infer<typeof ContextLayerBudgetSchema>;

/** Budget status for one task (mirrors Rust `BudgetReport`, camelCase). */
export const ContextBudgetStatusSchema = z
  .object({
    taskId: TaskIdSchema,
    totalUsed: z.number().int().nonnegative(),
    totalBudget: z.number().int().nonnegative(),
    overflow: z.boolean(),
    layers: z.array(ContextLayerBudgetSchema).max(16),
  })
  .strict();
export type ContextBudgetStatus = z.infer<typeof ContextBudgetStatusSchema>;

/** One compacted segment (mirrors Rust `CompactedSegment`, camelCase). */
export const ContextCompactedSegmentSchema = z
  .object({
    id: TalosIdSchema,
    layer: ContextLayerSchema,
    body: z.string().max(20000),
    originalTokens: z.number().int().nonnegative(),
    compactedTokens: z.number().int().nonnegative(),
    trimmed: z.boolean(),
    dropped: z.boolean(),
    protected: z.boolean(),
  })
  .strict();
export type ContextCompactedSegment = z.infer<
  typeof ContextCompactedSegmentSchema
>;

/** Deterministic TypeCompact outcome (mirrors Rust `CompactionResult`). */
export const ContextCompactionReceiptSchema = z
  .object({
    id: TalosIdSchema,
    taskId: TaskIdSchema,
    originalTokens: z.number().int().nonnegative(),
    compactedTokens: z.number().int().nonnegative(),
    preservedInvariants: z.array(z.string().min(1).max(128)).max(32),
    droppedSegments: z.array(z.string().min(1).max(128)).max(128),
    trimmedSegments: z.array(z.string().min(1).max(128)).max(128),
    proofHash: z.string().min(1).max(256),
    segments: z.array(ContextCompactedSegmentSchema).max(256),
    evidenceId: z.string().min(1).max(128).optional(),
    version: z.string().min(1).max(64),
    executedAt: IsoDateTimeSchema,
  })
  .strict();
export type ContextCompactionReceipt = z.infer<
  typeof ContextCompactionReceiptSchema
>;
