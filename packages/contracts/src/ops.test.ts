import { describe, expect, it } from "vitest";
import {
  CostBreakdownSchema,
  OperationalMetricsSchema,
  SpanSummarySchema,
  TelemetryExportSchema,
} from "./ops.js";

describe("ops schemas", () => {
  it("parses operational metrics", () => {
    const metrics = {
      totalTasks: 10,
      completedTasks: 7,
      failedTasks: 2,
      failureReasons: [{ reason: "timeout", count: 2 }],
      successRate: 0.7,
      failureRate: 0.2,
      avgTaskDurationMs: 1200,
      policyEvaluations: 20,
      policyDenials: 3,
      policyDenialRate: 0.15,
      modelLatency: { p50Ms: 100, p95Ms: 400, p99Ms: 900, samples: 5 },
      perModel: [],
      perTool: [],
      chainChecked: 12,
      chainVerifyMs: 1.5,
      totalTokens: 5000,
      sessionCostUsd: 0.01,
      capturedAt: new Date().toISOString(),
    } as const;
    expect(OperationalMetricsSchema.parse(metrics)).toEqual(metrics);
    expect(() =>
      OperationalMetricsSchema.parse({ ...metrics, prompt: "leaked" }),
    ).toThrow();
  });

  it("parses cost breakdowns and span summaries", () => {
    const breakdown = {
      sessionTotalUsd: 0.02,
      sessionBudgetUsd: 1,
      budgetConsumed: 0.02,
      byTask: [{ key: "t-1", costUsd: 0.02, records: 2 }],
      byAgent: [],
      byModel: [{ key: "openai/gpt-4o-mini", costUsd: 0.02, records: 2 }],
      byTool: [],
      capturedAt: new Date().toISOString(),
    } as const;
    expect(CostBreakdownSchema.parse(breakdown)).toEqual(breakdown);

    const span = {
      name: "talos.policy_broker.evaluate",
      component: "policy_broker",
      operation: "evaluate",
      durationMs: 3,
      status: "ok",
      statusMessage: "Allow",
      startTime: new Date().toISOString(),
      taskId: "t-1",
    } as const;
    expect(SpanSummarySchema.parse(span)).toEqual(span);
    expect(() =>
      SpanSummarySchema.parse({ ...span, status: "maybe" }),
    ).toThrow();

    const exported = {
      spansWritten: 4,
      path: "/tmp/talos-traces-x.json",
      exportedAt: new Date().toISOString(),
    } as const;
    expect(TelemetryExportSchema.parse(exported)).toEqual(exported);
  });
});
