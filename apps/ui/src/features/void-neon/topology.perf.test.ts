import { describe, expect, it } from "vitest";
import {
  AgentIdSchema,
  AgentStatusSchema,
  SessionIdSchema,
  TaskIdSchema,
  type AgentDescriptor,
} from "@talos/contracts";
import {
  applyKindFilter,
  buildLiveTopology,
  countFeedSignals,
  KIND_FILTER_GROUPS,
  type TopologyTask,
} from "./topology.js";

const STATUSES = ["running", "waiting_policy", "failed", "completed", "idle"];

/** Worst-case-ish fixed input: 24 agents, 48 tasks, 200 feed events. */
function fixedInput(): {
  agents: AgentDescriptor[];
  tasks: TopologyTask[];
  toolIds: string[];
  liveFeed: Array<{
    id: string;
    actor: string;
    message: string;
    taskId?: string;
    at: string;
  }>;
} {
  const agents: AgentDescriptor[] = Array.from({ length: 24 }, (_, i) => ({
    id: AgentIdSchema.parse(`agent-${i}`),
    displayName: `Agent ${i}`,
    role: "coder",
    status: AgentStatusSchema.parse(STATUSES[i % STATUSES.length]),
    sessionId: SessionIdSchema.parse(`session-${i}`),
    taskId: TaskIdSchema.parse(`task-${i % 48}`),
    capabilities: ["fs.read"],
    createdAt: "2026-11-13T00:00:00.000Z",
    updatedAt: "2026-11-13T00:00:00.000Z",
  }));
  const taskStatuses = [
    "executing",
    "queued",
    "waiting_policy",
    "failed",
    "completed",
  ] as const;
  const tasks: TopologyTask[] = Array.from({ length: 48 }, (_, i) => ({
    id: `task-${i}`,
    title: `Task ${i}`,
    status: taskStatuses[i % taskStatuses.length] ?? "executing",
    parentTaskId: null,
    executionFidelity: "mock",
  }));
  const toolIds = Array.from({ length: 40 }, (_, i) => `fs.tool${i}`);
  const liveFeed = Array.from({ length: 200 }, (_, i) => ({
    id: `event-${i}`,
    actor: i % 2 === 0 ? "browser" : "computer",
    message: `event ${i}`,
    taskId: `task-${i % 48}`,
    at: "2026-11-13T00:00:00.000Z",
  }));
  return { agents, tasks, toolIds, liveFeed };
}

function percentile(sorted: number[], p: number): number {
  return (
    sorted[
      Math.min(sorted.length - 1, Math.floor((p / 100) * sorted.length))
    ] ?? 0
  );
}

/** Opt-in timing log (`TALOS_PERF_LOG=1`): real numbers for docs/PERFORMANCE.md. */
function logSample(name: string, samples: number[]): void {
  if (process.env.TALOS_PERF_LOG !== "1") {
    return;
  }
  const p50 = percentile(samples, 50);
  const p95 = percentile(samples, 95);
  console.log(
    `[talos-perf] ${name} n=${samples.length} p50=${p50.toFixed(3)}ms p95=${p95.toFixed(3)}ms`,
  );
}

describe("void-neon topology performance (deterministic)", () => {
  it("builds the worst-case topology within budget", () => {
    const { agents, tasks, toolIds } = fixedInput();
    const samples: number[] = [];
    for (let i = 0; i < 25; i += 1) {
      const start = performance.now();
      const topology = buildLiveTopology({
        agents,
        tasks,
        toolIds,
        computerStopped: false,
        policyDecision: null,
        evidenceCount: 200,
      });
      applyKindFilter(
        topology,
        new Set(KIND_FILTER_GROUPS.map(({ kind }) => kind)),
      );
      samples.push(performance.now() - start);
    }
    samples.sort((a, b) => a - b);
    const p95 = percentile(samples, 95);
    logSample("topology.build+filter", samples);
    // Regression tripwire only (generous headroom): the P3 interaction
    // budget (≤100ms) is gated by the harness + CI perf job; this pins the
    // selector path far below it on any machine.
    expect(p95).toBeLessThan(100);
  });

  it("commits a 200-event feed window within budget", () => {
    const { liveFeed } = fixedInput();
    const samples: number[] = [];
    for (let i = 0; i < 25; i += 1) {
      const start = performance.now();
      countFeedSignals(liveFeed);
      samples.push(performance.now() - start);
    }
    samples.sort((a, b) => a - b);
    logSample("feed.commit", samples);
    expect(percentile(samples, 95)).toBeLessThan(50);
  });
});
