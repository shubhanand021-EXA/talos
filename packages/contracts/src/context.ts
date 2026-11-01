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

/** Assembly budget allocation (percentages must sum to 100). */
export const AssemblyAllocationSchema = z
  .object({
    contractsPct: z.number().int().min(0).max(100),
    toolStatePct: z.number().int().min(0).max(100),
    evidencePct: z.number().int().min(0).max(100),
    memoryPct: z.number().int().min(0).max(100),
    planPct: z.number().int().min(0).max(100),
  })
  .strict()
  .refine(
    (allocation) =>
      allocation.contractsPct +
        allocation.toolStatePct +
        allocation.evidencePct +
        allocation.memoryPct +
        allocation.planPct ===
      100,
    { message: "allocation buckets must sum to 100" },
  );
export type AssemblyAllocation = z.infer<typeof AssemblyAllocationSchema>;

/** Assembly knobs (mirrors Rust `ContextAssemblyConfig`). */
export const ContextAssemblyConfigSchema = z
  .object({
    allocation: AssemblyAllocationSchema,
    reservedOutputPct: z.number().int().min(0).max(50),
    templateVersion: z.string().min(1).max(64),
    defaultContextWindow: z.number().int().positive(),
  })
  .strict();
export type ContextAssemblyConfig = z.infer<typeof ContextAssemblyConfigSchema>;

/** One compaction applied during assembly (mirrors `CompactionSummary`). */
export const AssemblyCompactionSchema = z
  .object({
    layer: ContextLayerSchema,
    originalTokens: z.number().int().nonnegative(),
    compactedTokens: z.number().int().nonnegative(),
    proofHash: z.string().min(1).max(256),
    preservedInvariants: z.array(z.string().min(1).max(256)).max(32),
    degraded: z.boolean(),
  })
  .strict();
export type AssemblyCompaction = z.infer<typeof AssemblyCompactionSchema>;

/** Per-layer token accounting for one assembly. */
export const AssembledLayerSchema = z
  .object({
    layer: ContextLayerSchema,
    usedTokens: z.number().int().nonnegative(),
    targetTokens: z.number().int().nonnegative(),
    compacted: z.boolean(),
  })
  .strict();
export type AssembledLayer = z.infer<typeof AssembledLayerSchema>;

/**
 * Assembled context metadata (mirrors Rust `AssembledContext` minus the
 * prompt text: structure, counts, and hashes only — never message bodies).
 */
export const AssembledContextMetadataSchema = z
  .object({
    taskId: TaskIdSchema,
    model: z.string().min(1).max(128),
    contextWindow: z.number().int().positive(),
    totalTokens: z.number().int().nonnegative(),
    layers: z.array(AssembledLayerSchema).max(16),
    toolsIncluded: z.array(z.string().min(1).max(128)).max(64),
    evidenceRefs: z.array(z.string().min(1).max(128)).max(256),
    templateVersion: z.string().min(1).max(64),
    estimatorUsed: z.string().min(1).max(64),
    assemblyHash: z.string().min(1).max(256),
    compactionsApplied: z.array(AssemblyCompactionSchema).max(16),
    degraded: z.boolean(),
    executedAt: IsoDateTimeSchema,
  })
  .strict();
export type AssembledContextMetadata = z.infer<
  typeof AssembledContextMetadataSchema
>;
