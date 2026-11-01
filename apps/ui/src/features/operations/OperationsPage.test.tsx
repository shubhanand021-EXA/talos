import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { OperationsPage } from "./OperationsPage.js";

const { mockMetrics, mockCosts, mockTraces, mockExport, mockBudget } =
  vi.hoisted(() => ({
    mockMetrics: vi.fn(),
    mockCosts: vi.fn(),
    mockTraces: vi.fn(),
    mockExport: vi.fn(),
    mockBudget: vi.fn(),
  }));

vi.mock("../../bridge/index.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../bridge/index.js")>();
  return {
    ...actual,
    isTauri: () => true,
    getOperationalMetrics: mockMetrics,
    getCostBreakdown: mockCosts,
    getTraceSummary: mockTraces,
    exportTelemetry: mockExport,
    setSessionBudget: mockBudget,
  };
});

const METRICS = {
  totalTasks: 10,
  completedTasks: 7,
  failedTasks: 2,
  failureReasons: [{ reason: "worker error: timeout", count: 2 }],
  successRate: 0.7,
  failureRate: 0.2,
  avgTaskDurationMs: 1200,
  policyEvaluations: 20,
  policyDenials: 3,
  policyDenialRate: 0.15,
  modelLatency: { p50Ms: 100, p95Ms: 400, p99Ms: 900, samples: 5 },
  perModel: [
    {
      providerId: "openai",
      modelId: "gpt-4o-mini",
      calls: 5,
      p50Ms: 100,
      p95Ms: 400,
      p99Ms: 900,
      costUsd: 0.01,
    },
  ],
  perTool: [{ toolId: "fs.read", success: 9, failure: 1, successRate: 0.9 }],
  chainChecked: 12,
  chainVerifyMs: 1.5,
  totalTokens: 5000,
  sessionCostUsd: 0.012,
  capturedAt: new Date().toISOString(),
};

const COSTS = {
  sessionTotalUsd: 0.012,
  sessionBudgetUsd: 1,
  budgetConsumed: 0.012,
  byTask: [{ key: "t-1", costUsd: 0.012, records: 2 }],
  byAgent: [{ key: "a-1", costUsd: 0.012, records: 2 }],
  byModel: [{ key: "openai/gpt-4o-mini", costUsd: 0.01, records: 5 }],
  byTool: [{ key: "fs.read", costUsd: 0.002, records: 9 }],
  capturedAt: new Date().toISOString(),
};

const TRACES = [
  {
    name: "talos.policy_broker.evaluate",
    component: "policy_broker",
    operation: "evaluate",
    durationMs: 3,
    status: "ok",
    statusMessage: "Allow",
    startTime: new Date().toISOString(),
    taskId: "t-1",
  },
];

beforeEach(() => {
  mockMetrics.mockReset().mockResolvedValue(METRICS);
  mockCosts.mockReset().mockResolvedValue(COSTS);
  mockTraces.mockReset().mockResolvedValue(TRACES);
  mockExport.mockReset().mockResolvedValue({
    spansWritten: 1,
    path: "/tmp/talos-traces-x.json",
    exportedAt: new Date().toISOString(),
  });
  mockBudget.mockReset().mockResolvedValue(5);
});

describe("OperationsPage", () => {
  it("renders metrics, cost breakdown, and traces", async () => {
    render(<OperationsPage />);
    const rates = await screen.findAllByText("70.0%");
    expect(rates.length).toBeGreaterThan(0);
    expect(screen.getAllByText("openai/gpt-4o-mini").length).toBeGreaterThan(0);
    expect(
      screen.getByText("talos.policy_broker.evaluate"),
    ).toBeInTheDocument();
    expect(screen.getAllByText("fs.read").length).toBeGreaterThan(0);
    expect(screen.getByText("worker error: timeout")).toBeInTheDocument();
  });

  it("exports traces on demand", async () => {
    const user = userEvent.setup();
    render(<OperationsPage />);
    await user.click(
      await screen.findByRole("button", { name: "Export traces" }),
    );
    expect(await screen.findByText(/Exported 1 spans/)).toBeInTheDocument();
    expect(mockExport).toHaveBeenCalled();
  });

  it("sets the session budget", async () => {
    const user = userEvent.setup();
    render(<OperationsPage />);
    const input = await screen.findByLabelText("Session budget in USD");
    await user.clear(input);
    await user.type(input, "5");
    await user.click(screen.getByRole("button", { name: "Set budget" }));
    expect(mockBudget).toHaveBeenCalledWith(5);
    expect(await screen.findByText(/Session budget set/)).toBeInTheDocument();
  });
});
