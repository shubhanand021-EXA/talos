import { describe, expect, it } from "vitest";
import { ToolCandidateSchema, ToolRagQuerySchema } from "./toolrag.js";

describe("tool-rag domain", () => {
  it("parses ranked candidates with score and reason", () => {
    const candidate = {
      toolId: "fs.read",
      score: 4.5,
      reason: "keyword overlap 2 token(s); risk Low within ceiling any",
      matchedFields: ["name", "description"],
      riskLevel: "Low",
      requiredApprovals: [],
    } as const;
    expect(ToolCandidateSchema.parse(candidate)).toEqual(candidate);
    expect(() =>
      ToolCandidateSchema.parse({ ...candidate, score: -1 }),
    ).toThrow();
  });

  it("parses retrieval queries with filters", () => {
    const query = {
      intent: "read a file",
      capabilities: ["read_file"],
      riskCeiling: "medium",
      requireEvidence: false,
      topK: 5,
    } as const;
    expect(ToolRagQuerySchema.parse(query)).toEqual(query);
    expect(() =>
      ToolRagQuerySchema.parse({ ...query, riskCeiling: "extreme" }),
    ).toThrow();
    expect(() => ToolRagQuerySchema.parse({ intent: "" })).toThrow();
  });
});
