import { z } from "zod";
import { IsoDateTimeSchema, TaskIdSchema } from "./identity.js";

/** Memory source types indexed for semantic recall. */
export const MemorySourceTypeSchema = z.enum([
  "claim",
  "evidence",
  "task",
  "skill",
]);
export type MemorySourceType = z.infer<typeof MemorySourceTypeSchema>;

/** Retrieval mode for memory and tool search. */
export const RetrievalModeSchema = z.enum(["lexical", "semantic", "hybrid"]);
export type RetrievalMode = z.infer<typeof RetrievalModeSchema>;

/** One ranked memory hit (mirrors Rust `SearchResult`, camelCase). */
export const MemorySearchResultSchema = z
  .object({
    id: z.string().min(1).max(256),
    sourceType: MemorySourceTypeSchema,
    sourceId: z.string().min(1).max(128),
    taskId: TaskIdSchema.nullish(),
    score: z.number().min(0).max(1),
    lexicalScore: z.number().min(0).max(1),
    semanticScore: z.number().min(-1).max(1),
    snippet: z.string().max(2048),
    verified: z.boolean(),
    createdAt: IsoDateTimeSchema,
    reason: z.string().min(1).max(2048),
  })
  .strict();
export type MemorySearchResult = z.infer<typeof MemorySearchResultSchema>;

/** Query shape for `search_memory`. */
export const MemoryQuerySchema = z
  .object({
    query: z.string().min(1).max(1024),
    topK: z.number().int().min(1).max(50).optional(),
    sourceTypes: z.array(MemorySourceTypeSchema).max(4).optional(),
    taskId: TaskIdSchema.optional(),
    verifiedOnly: z.boolean().optional(),
    mode: RetrievalModeSchema.optional(),
  })
  .strict();
export type MemoryQuery = z.infer<typeof MemoryQuerySchema>;

/** Per-source-type index count. */
export const IndexSourceCountSchema = z
  .object({
    sourceType: z.string().min(1).max(32),
    count: z.number().int().nonnegative(),
  })
  .strict();
export type IndexSourceCount = z.infer<typeof IndexSourceCountSchema>;

/** Index status for `get_index_status` / `rebuild_index`. */
export const IndexStatusSchema = z
  .object({
    entries: z.number().int().nonnegative(),
    bySourceType: z.array(IndexSourceCountSchema).max(8),
    backend: z.string().min(1).max(64),
    dims: z.number().int().positive(),
    modelLoaded: z.boolean(),
    modelDir: z.string().min(1).max(1024),
    warning: z.string().max(1024),
    lastIndexedAt: IsoDateTimeSchema.nullish(),
  })
  .strict();
export type IndexStatus = z.infer<typeof IndexStatusSchema>;

/** Skill lifecycle status. */
export const SkillStatusSchema = z.enum([
  "quarantined",
  "validated",
  "published",
]);
export type SkillStatus = z.infer<typeof SkillStatusSchema>;

/** Skill registry row (mirrors Rust `SkillDto`, camelCase). */
export const SkillSchema = z
  .object({
    id: z.string().min(1).max(128),
    name: z.string().min(1).max(256),
    description: z.string().max(2048),
    version: z.string().min(1).max(32),
    status: SkillStatusSchema,
    sourceTaskId: TaskIdSchema.nullish(),
    permissionCeiling: z.string().min(1).max(32),
    requiredTools: z.array(z.string().min(1).max(64)).max(32),
    createdAt: IsoDateTimeSchema,
  })
  .strict();
export type Skill = z.infer<typeof SkillSchema>;

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
