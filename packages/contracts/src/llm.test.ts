import { describe, expect, it } from "vitest";
import {
  LlmRequestMetadataSchema,
  LlmRequestSchema,
  ModelDescriptorSchema,
  ProviderIdSchema,
  ProviderReceiptSchema,
  ProviderStatusSchema,
  TokenUsageSchema,
  UsageSummarySchema,
} from "./llm.js";

describe("llm domain", () => {
  it("parses provider ids", () => {
    expect(ProviderIdSchema.parse("openai")).toBe("openai");
    expect(ProviderIdSchema.parse("ollama")).toBe("ollama");
    expect(() => ProviderIdSchema.parse("skynet")).toThrow();
  });

  it("parses requests with required linkage", () => {
    const request = {
      model: "gpt-4o-mini",
      messages: [{ role: "user", content: "hi" }],
      metadata: { taskId: "task-1", agentId: "agent-1", purpose: "test" },
    } as const;
    expect(LlmRequestSchema.parse(request)).toMatchObject({
      model: "gpt-4o-mini",
      stopSequences: [],
    });
    expect(() =>
      LlmRequestSchema.parse({ ...request, messages: [] }),
    ).toThrow();
    expect(() =>
      LlmRequestSchema.parse({
        ...request,
        metadata: { taskId: "", agentId: "agent-1", purpose: "test" },
      }),
    ).toThrow();
  });

  it("parses descriptors, receipts, and usage", () => {
    const descriptor = {
      providerId: "openai",
      modelId: "gpt-4o-mini",
      contextWindow: 128000,
      supportsTools: true,
      supportsVision: true,
      supportsStreaming: true,
      costPer1kInput: 0.00015,
      costPer1kOutput: 0.0006,
    } as const;
    expect(ModelDescriptorSchema.parse(descriptor)).toEqual(descriptor);

    const receipt = {
      providerId: "openai",
      modelId: "gpt-4o-mini",
      usage: { promptTokens: 10, completionTokens: 5, totalTokens: 15 },
      latencyMs: 120,
      estimatedCostUsd: 0.0000045,
    } as const;
    expect(ProviderReceiptSchema.parse(receipt)).toEqual(receipt);
    expect(TokenUsageSchema.parse(receipt.usage)).toEqual(receipt.usage);

    const status = {
      providerId: "ollama",
      configured: true,
      reachable: true,
      latencyMs: 12,
      models: ["llama3"],
    } as const;
    expect(ProviderStatusSchema.parse(status)).toEqual(status);

    const summary = {
      totalTokens: 15,
      estimatedCostUsd: 0.0000045,
      callCount: 1,
      byProvider: [
        {
          providerId: "openai",
          totalTokens: 15,
          estimatedCostUsd: 0.0000045,
          callCount: 1,
        },
      ],
    } as const;
    expect(UsageSummarySchema.parse(summary)).toEqual(summary);
  });

  it("parses request metadata linkage", () => {
    expect(
      LlmRequestMetadataSchema.parse({
        taskId: "task-1",
        agentId: "agent-1",
        purpose: "agent task execution",
      }),
    ).toMatchObject({ purpose: "agent task execution" });
  });
});
