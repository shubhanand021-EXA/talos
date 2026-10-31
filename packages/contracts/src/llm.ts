import { z } from "zod";
import { AgentIdSchema, IsoDateTimeSchema, TaskIdSchema } from "./identity.js";

export const ProviderIdSchema = z.enum([
  "openai",
  "anthropic",
  "ollama",
  "custom",
]);
export type ProviderId = z.infer<typeof ProviderIdSchema>;

export const LlmRoleSchema = z.enum(["system", "user", "assistant", "tool"]);
export type LlmRole = z.infer<typeof LlmRoleSchema>;

export const LlmMessageSchema = z
  .object({
    role: LlmRoleSchema,
    content: z.string().min(1).max(65536),
    toolCallId: z.string().min(1).max(128).optional(),
  })
  .strict();
export type LlmMessage = z.infer<typeof LlmMessageSchema>;

export const LlmToolSchemaSchema = z
  .object({
    name: z.string().min(1).max(128),
    description: z.string().min(1).max(2048),
    inputSchema: z.unknown(),
  })
  .strict();
export type LlmToolSchema = z.infer<typeof LlmToolSchemaSchema>;

export const LlmRequestMetadataSchema = z
  .object({
    taskId: TaskIdSchema,
    agentId: AgentIdSchema,
    purpose: z.string().min(1).max(256),
  })
  .strict();
export type LlmRequestMetadata = z.infer<typeof LlmRequestMetadataSchema>;

export const LlmRequestSchema = z
  .object({
    model: z.string().min(1).max(128),
    messages: z.array(LlmMessageSchema).min(1).max(256),
    tools: z.array(LlmToolSchemaSchema).max(64).optional(),
    temperature: z.number().min(0).max(2).optional(),
    maxTokens: z.number().int().min(1).max(200000).optional(),
    stopSequences: z.array(z.string().min(1).max(128)).max(8).default([]),
    metadata: LlmRequestMetadataSchema,
  })
  .strict();
export type LlmRequest = z.infer<typeof LlmRequestSchema>;

export const TokenUsageSchema = z
  .object({
    promptTokens: z.number().int().min(0),
    completionTokens: z.number().int().min(0),
    totalTokens: z.number().int().min(0),
  })
  .strict();
export type TokenUsage = z.infer<typeof TokenUsageSchema>;

export const FinishReasonSchema = z.enum([
  "stop",
  "length",
  "tool_calls",
  "content_filter",
]);
export type FinishReason = z.infer<typeof FinishReasonSchema>;

export const LlmToolCallSchema = z
  .object({
    id: z.string().min(1).max(128),
    name: z.string().min(1).max(128),
    arguments: z.unknown(),
  })
  .strict();
export type LlmToolCall = z.infer<typeof LlmToolCallSchema>;

export const ModelDescriptorSchema = z
  .object({
    providerId: ProviderIdSchema,
    modelId: z.string().min(1).max(128),
    contextWindow: z.number().int().min(1),
    supportsTools: z.boolean(),
    supportsVision: z.boolean(),
    supportsStreaming: z.boolean(),
    costPer1kInput: z.number().min(0),
    costPer1kOutput: z.number().min(0),
  })
  .strict();
export type ModelDescriptor = z.infer<typeof ModelDescriptorSchema>;

export const ProviderStatusSchema = z
  .object({
    providerId: ProviderIdSchema,
    configured: z.boolean(),
    reachable: z.boolean(),
    latencyMs: z.number().int().min(0).nullable(),
    models: z.array(z.string().min(1).max(128)),
  })
  .strict();
export type ProviderStatus = z.infer<typeof ProviderStatusSchema>;

export const ProviderReceiptSchema = z
  .object({
    providerId: z.string().min(1).max(64),
    modelId: z.string().min(1).max(128),
    usage: TokenUsageSchema,
    latencyMs: z.number().int().min(0),
    estimatedCostUsd: z.number().min(0),
  })
  .strict();
export type ProviderReceipt = z.infer<typeof ProviderReceiptSchema>;

export const UsageRecordSchema = z
  .object({
    id: z.string().min(1).max(128),
    taskId: TaskIdSchema,
    agentId: AgentIdSchema,
    providerId: z.string().min(1).max(64),
    modelId: z.string().min(1).max(128),
    promptTokens: z.number().int().min(0),
    completionTokens: z.number().int().min(0),
    totalTokens: z.number().int().min(0),
    estimatedCostUsd: z.number().min(0),
    latencyMs: z.number().int().min(0),
    createdAt: IsoDateTimeSchema,
  })
  .strict();
export type UsageRecord = z.infer<typeof UsageRecordSchema>;

export const UsageSummarySchema = z
  .object({
    totalTokens: z.number().int().min(0),
    estimatedCostUsd: z.number().min(0),
    callCount: z.number().int().min(0),
    byProvider: z.array(
      z
        .object({
          providerId: z.string().min(1).max(64),
          totalTokens: z.number().int().min(0),
          estimatedCostUsd: z.number().min(0),
          callCount: z.number().int().min(0),
        })
        .strict(),
    ),
  })
  .strict();
export type UsageSummary = z.infer<typeof UsageSummarySchema>;
