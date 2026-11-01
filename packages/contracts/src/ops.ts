import { z } from "zod";

/** One latency percentile triple (all providers or per model). */
export const LatencyStatsSchema = z
  .object({
    p50Ms: z.number().int().min(0),
    p95Ms: z.number().int().min(0),
    p99Ms: z.number().int().min(0),
    samples: z.number().int().min(0),
  })
  .strict();
export type LatencyStats = z.infer<typeof LatencyStatsSchema>;

/** Per-model latency distribution + cost. */
export const ModelLatencySchema = z
  .object({
    providerId: z.string().min(1).max(64),
    modelId: z.string().min(1).max(128),
    calls: z.number().int().min(0),
    p50Ms: z.number().int().min(0),
    p95Ms: z.number().int().min(0),
    p99Ms: z.number().int().min(0),
    costUsd: z.number().min(0),
  })
  .strict();
export type ModelLatency = z.infer<typeof ModelLatencySchema>;

/** Per-tool success/failure counters. */
export const ToolStatsSchema = z
  .object({
    toolId: z.string().min(1).max(256),
    success: z.number().int().min(0),
    failure: z.number().int().min(0),
    successRate: z.number().min(0).max(1),
  })
  .strict();
export type ToolStats = z.infer<typeof ToolStatsSchema>;

/** One failure reason with its occurrence count. */
export const FailureReasonSchema = z
  .object({
    reason: z.string().min(1).max(256),
    count: z.number().int().min(1),
  })
  .strict();
export type FailureReason = z.infer<typeof FailureReasonSchema>;

/**
 * Dashboard-grade operational metrics (`get_operational_metrics`).
 * All values are computed locally from the evidence store; no external calls.
 */
export const OperationalMetricsSchema = z
  .object({
    totalTasks: z.number().int().min(0),
    completedTasks: z.number().int().min(0),
    failedTasks: z.number().int().min(0),
    failureReasons: z.array(FailureReasonSchema).max(5),
    successRate: z.number().min(0).max(1),
    failureRate: z.number().min(0).max(1),
    avgTaskDurationMs: z.number().min(0),
    policyEvaluations: z.number().int().min(0),
    policyDenials: z.number().int().min(0),
    policyDenialRate: z.number().min(0).max(1),
    modelLatency: LatencyStatsSchema,
    perModel: z.array(ModelLatencySchema).max(128),
    perTool: z.array(ToolStatsSchema).max(256),
    chainChecked: z.number().int().min(0),
    chainVerifyMs: z.number().min(0),
    totalTokens: z.number().int().min(0),
    sessionCostUsd: z.number().min(0),
    capturedAt: z.string().min(1),
  })
  .strict();
export type OperationalMetrics = z.infer<typeof OperationalMetricsSchema>;

/** One cost attribution slice (task, agent, model, or tool). */
export const CostSliceSchema = z
  .object({
    key: z.string().min(1).max(256),
    costUsd: z.number().min(0),
    records: z.number().int().min(0),
  })
  .strict();
export type CostSlice = z.infer<typeof CostSliceSchema>;

/**
 * Full cost breakdown (`get_cost_breakdown`): session total plus
 * attribution by task / agent / model / tool and budget consumption.
 */
export const CostBreakdownSchema = z
  .object({
    sessionTotalUsd: z.number().min(0),
    sessionBudgetUsd: z.number().min(0).nullable(),
    budgetConsumed: z.number().min(0).max(1),
    byTask: z.array(CostSliceSchema).max(1000),
    byAgent: z.array(CostSliceSchema).max(1000),
    byModel: z.array(CostSliceSchema).max(256),
    byTool: z.array(CostSliceSchema).max(256),
    capturedAt: z.string().min(1),
  })
  .strict();
export type CostBreakdown = z.infer<typeof CostBreakdownSchema>;

/**
 * Span summary (`get_trace_summary`): metadata only — name, duration,
 * status, task/agent linkage. Never payloads, prompts, or screenshots.
 */
export const SpanSummarySchema = z
  .object({
    name: z.string().min(1).max(256),
    component: z.string().min(1).max(128),
    operation: z.string().min(1).max(128),
    durationMs: z.number().int().min(0),
    status: z.enum(["ok", "error"]),
    statusMessage: z.string().max(256),
    startTime: z.string().min(1),
    taskId: z.string().min(1).max(128).nullish(),
    agentId: z.string().min(1).max(128).nullish(),
  })
  .strict();
export type SpanSummary = z.infer<typeof SpanSummarySchema>;

/** Result of an explicit local telemetry export (`export_telemetry`). */
export const TelemetryExportSchema = z
  .object({
    spansWritten: z.number().int().min(0),
    path: z.string().min(1).max(1024),
    exportedAt: z.string().min(1),
  })
  .strict();
export type TelemetryExport = z.infer<typeof TelemetryExportSchema>;
