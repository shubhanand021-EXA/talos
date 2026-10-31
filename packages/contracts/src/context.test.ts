import { describe, expect, it } from "vitest";
import {
  ContextBudgetStatusSchema,
  ContextCompactionReceiptSchema,
  ContextLayerSchema,
} from "./context.js";
import { TaskIdSchema } from "./identity.js";

const NOW = "2026-10-31T00:00:00.000Z";

describe("context domain", () => {
  it("accepts all seven layers", () => {
    for (const layer of [
      "system",
      "session",
      "task",
      "evidence",
      "tool",
      "scratchpad",
      "safety",
    ] as const) {
      expect(ContextLayerSchema.parse(layer)).toBe(layer);
    }
    expect(() => ContextLayerSchema.parse("llm")).toThrow();
  });

  it("parses budget status with per-layer usage", () => {
    const status = {
      taskId: TaskIdSchema.parse("task-1"),
      totalUsed: 120,
      totalBudget: 8000,
      overflow: false,
      layers: [
        {
          layer: "safety",
          usedTokens: 10,
          maxTokens: 800,
          overflow: false,
          protected: true,
          segmentCount: 1,
        },
      ],
    } as const;
    expect(ContextBudgetStatusSchema.parse(status)).toEqual(status);
  });

  it("parses compaction receipts with proof hash and invariants", () => {
    const receipt = {
      id: "compact-task-1-abc",
      taskId: TaskIdSchema.parse("task-1"),
      originalTokens: 200,
      compactedTokens: 120,
      preservedInvariants: ["ids:3/3", "safety:unchanged"],
      droppedSegments: [],
      trimmedSegments: ["scratch-task-1"],
      proofHash: "sha256:abc",
      segments: [
        {
          id: "sys-task-1",
          layer: "system",
          body: "system prompt",
          originalTokens: 4,
          compactedTokens: 4,
          trimmed: false,
          dropped: false,
          protected: true,
        },
      ],
      version: "typecompact/1",
      executedAt: NOW,
    } as const;
    expect(ContextCompactionReceiptSchema.parse(receipt)).toEqual(receipt);
    expect(() =>
      ContextCompactionReceiptSchema.parse({ ...receipt, proofHash: "" }),
    ).toThrow();
  });
});
