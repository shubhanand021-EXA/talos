import { z } from "zod";

/** One ranked tool candidate (mirrors Rust `ToolCandidate`, camelCase). */
export const ToolCandidateSchema = z
  .object({
    toolId: z.string().min(1).max(128),
    score: z.number().nonnegative(),
    reason: z.string().min(1).max(2048),
    matchedFields: z.array(z.string().min(1).max(64)).max(8),
    riskLevel: z.string().min(1).max(32),
    requiredApprovals: z.array(z.string().min(1).max(64)).max(8),
  })
  .strict();
export type ToolCandidate = z.infer<typeof ToolCandidateSchema>;

/** Retrieval request shape for `query_tool_candidates`. */
export const ToolRagQuerySchema = z
  .object({
    intent: z.string().min(1).max(1024),
    capabilities: z.array(z.string().min(1).max(64)).max(16).optional(),
    riskCeiling: z.enum(["low", "medium", "high", "irreversible"]).optional(),
    requireEvidence: z.boolean().optional(),
    topK: z.number().int().min(1).max(20).optional(),
  })
  .strict();
export type ToolRagQuery = z.infer<typeof ToolRagQuerySchema>;
