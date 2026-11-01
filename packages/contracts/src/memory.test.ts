import { describe, expect, it } from "vitest";
import {
  IndexStatusSchema,
  MemoryQuerySchema,
  MemorySearchResultSchema,
  SkillSchema,
} from "./memory.js";

const NOW = "2026-10-31T00:00:00.000Z";

describe("memory contracts", () => {
  it("parses search results", () => {
    const hit = {
      id: "claim:claim-1",
      sourceType: "claim",
      sourceId: "claim-1",
      taskId: "task-1",
      score: 0.87,
      lexicalScore: 0.75,
      semanticScore: 0.92,
      snippet: "reads succeed with hash",
      verified: true,
      createdAt: NOW,
      reason: "hybrid blend (lexical 0.75, semantic 0.92); ref claim:claim-1",
    };
    expect(MemorySearchResultSchema.parse(hit)).toMatchObject({
      sourceType: "claim",
    });
    expect(() =>
      MemorySearchResultSchema.parse({ ...hit, score: 2 }),
    ).toThrow();
    expect(() =>
      MemorySearchResultSchema.parse({ ...hit, sourceType: "nope" }),
    ).toThrow();
  });

  it("parses memory queries with filters", () => {
    expect(MemoryQuerySchema.parse({ query: "read file hash" })).toMatchObject({
      query: "read file hash",
    });
    expect(
      MemoryQuerySchema.parse({
        query: "x",
        topK: 5,
        sourceTypes: ["claim", "skill"],
        verifiedOnly: true,
        mode: "hybrid",
      }),
    ).toMatchObject({ mode: "hybrid" });
    expect(() => MemoryQuerySchema.parse({ query: "" })).toThrow();
    expect(() =>
      MemoryQuerySchema.parse({ query: "x", mode: "nope" }),
    ).toThrow();
  });

  it("parses index status", () => {
    const status = {
      entries: 3,
      bySourceType: [
        { sourceType: "claim", count: 1 },
        { sourceType: "evidence", count: 2 },
      ],
      backend: "local-hash-384/1",
      dims: 384,
      modelLoaded: false,
      modelDir: ".talos/models",
      warning: "no ONNX runtime bundled",
      lastIndexedAt: NOW,
    };
    expect(IndexStatusSchema.parse(status)).toMatchObject({ entries: 3 });
  });

  it("parses skills", () => {
    const skill = {
      id: "skill-task-1",
      name: "read then hash",
      description: "Replay of task-1.",
      version: "0.1.0",
      status: "quarantined",
      sourceTaskId: "task-1",
      permissionCeiling: "low",
      requiredTools: ["fs.read"],
      createdAt: NOW,
    };
    expect(SkillSchema.parse(skill)).toMatchObject({ status: "quarantined" });
    expect(() => SkillSchema.parse({ ...skill, status: "archived" })).toThrow();
  });
});
