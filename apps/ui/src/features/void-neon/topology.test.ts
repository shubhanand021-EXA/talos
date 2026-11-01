import { describe, expect, it } from "vitest";
import { AgentDescriptorSchema, ToolIdSchema } from "@talos/contracts";
import {
  applyKindFilter,
  buildLiveTopology,
  buildTopology,
  countFeedSignals,
  describeNode,
  EDGE_STATE_COLORS,
  edgeColorFor,
  findNode,
  recentTaskIds,
  selectKindFor,
  STATUS_COLORS,
  toTaskTopologyStatus,
  toTopologyStatus,
  toolKindFor,
} from "./topology.js";

const STAMP = "2026-10-31T12:00:00.000Z";

function agent(
  id: string,
  status: "running" | "idle" | "waiting_policy" | "failed" | "completed",
  taskId?: string,
) {
  return AgentDescriptorSchema.parse({
    id,
    displayName: id,
    role: "coder",
    status,
    sessionId: "session-local",
    ...(taskId === undefined ? {} : { taskId }),
    capabilities: ["fs.read"],
    createdAt: STAMP,
    updatedAt: STAMP,
  });
}

function liveInput() {
  return {
    agents: [agent("a-1", "running", "task-1"), agent("a-2", "idle")],
    tasks: [
      { id: "task-1", title: "Do work", status: "executing" },
      { id: "task-2", title: "Wait here", status: "waiting_policy" },
    ],
    toolIds: [...ToolIdSchema.options],
    computerStopped: false,
    policyDecision: "allow" as string | null,
    evidenceCount: 3,
  };
}

describe("topology", () => {
  it("maps lifecycle statuses onto scene colors", () => {
    expect(toTopologyStatus("running")).toBe("running");
    expect(toTopologyStatus("starting")).toBe("running");
    expect(toTopologyStatus("waiting_policy")).toBe("waiting_policy");
    expect(toTopologyStatus("failed")).toBe("failed");
    expect(toTopologyStatus("completed")).toBe("completed");
    expect(toTopologyStatus("terminated")).toBe("completed");
    expect(toTopologyStatus("idle")).toBe("idle");
    expect(STATUS_COLORS.running).toBe("#22d3ee");
  });

  it("builds a deterministic core + agent graph", () => {
    const first = buildTopology([
      agent("a-1", "running"),
      agent("a-2", "idle"),
    ]);
    const second = buildTopology([
      agent("a-1", "running"),
      agent("a-2", "idle"),
    ]);
    expect(first.nodes).toHaveLength(3);
    expect(first.edges).toHaveLength(2);
    expect(first).toEqual(second);
    expect(first.nodes[0]?.label).toBe("Talos Core");
  });

  it("propagates store-driven status updates", () => {
    const before = buildTopology([agent("a-1", "running")]);
    const after = buildTopology([agent("a-1", "failed")]);
    const lifecycleOf = (topology: typeof before) =>
      (topology.nodes[1]?.metadata as { lifecycle?: string } | undefined)
        ?.lifecycle;
    expect(lifecycleOf(before)).toBe("running");
    expect(lifecycleOf(after)).toBe("failed");
    expect(edgeColorFor(after, after.edges[0]!)).toBe(STATUS_COLORS.failed);
  });

  it("positions agents on a deterministic ring", () => {
    const topology = buildTopology([
      agent("a-1", "idle"),
      agent("a-2", "idle"),
      agent("a-3", "idle"),
    ]);
    const positions = topology.nodes.slice(1).map((n) => n.position);
    expect(new Set(positions.map((p) => JSON.stringify(p))).size).toBe(3);
  });

  it("maps task queue states onto visuals", () => {
    expect(toTaskTopologyStatus("queued")).toBe("queued");
    expect(toTaskTopologyStatus("planning")).toBe("running");
    expect(toTaskTopologyStatus("executing")).toBe("running");
    expect(toTaskTopologyStatus("waiting_policy")).toBe("waiting_policy");
    expect(toTaskTopologyStatus("waiting_evidence")).toBe("waiting_policy");
    expect(toTaskTopologyStatus("denied")).toBe("denied");
    expect(toTaskTopologyStatus("failed")).toBe("failed");
    expect(toTaskTopologyStatus("timed_out")).toBe("failed");
    expect(toTaskTopologyStatus("completed")).toBe("completed");
    expect(toTaskTopologyStatus("canceled")).toBe("canceled");
    expect(toTaskTopologyStatus("bogus")).toBe("idle");
    expect(STATUS_COLORS.queued).toBe("#475569");
    expect(STATUS_COLORS.denied).toBe("#fb7185");
    expect(STATUS_COLORS.canceled).toBe("#6b7280");
    expect(STATUS_COLORS.stopped).toBe("#ef4444");
  });

  it("builds a live topology with every node kind", () => {
    const topology = buildLiveTopology(liveInput());
    const kinds = new Set(topology.nodes.map((node) => node.kind));
    for (const kind of [
      "system",
      "agent",
      "task",
      "tool",
      "browser",
      "computer-use",
      "policy",
      "evidence",
      "audit",
    ]) {
      expect(kinds.has(kind as never)).toBe(true);
    }
    expect(findNode(topology, "policy-gate")?.kind).toBe("policy");
    expect(findNode(topology, "evidence-ledger")?.kind).toBe("evidence");
    expect(findNode(topology, "audit-sentinel")?.kind).toBe("audit");
    // Deterministic: same input, same topology.
    expect(buildLiveTopology(liveInput())).toEqual(topology);
  });

  it("wires edges for assignment, policy, evidence, and invocation flows", () => {
    const topology = buildLiveTopology(liveInput());
    const byId = new Map<string, (typeof topology.edges)[number]>(
      topology.edges.map((edge) => [edge.id, edge]),
    );
    // Agent owns task-1: assignment edge from the agent.
    expect(byId.get("assign-task-1")?.source).toBe("a-1");
    expect(byId.get("assign-task-1")?.state).toBe("active");
    // Task-2 waits on policy: pending gate edge + amber color.
    expect(byId.get("gate-task-2")?.state).toBe("pending");
    expect(byId.get("gate-task-2")?.target).toBe("policy-gate");
    expect(edgeColorFor(topology, byId.get("gate-task-2")!)).toBe(
      EDGE_STATE_COLORS.pending,
    );
    // Invocation edges reach every tool family.
    expect(byId.get("invoke-fs.read")?.label).toBe("tool invocation");
    expect(byId.get("invoke-browser.click")?.label).toBe(
      "browser/computer action",
    );
    expect(byId.get("invoke-computer.click")?.label).toBe(
      "browser/computer action",
    );
    expect(toolKindFor("fs.read")).toBe("tool");
    expect(toolKindFor("browser.click")).toBe("browser");
    expect(toolKindFor("computer.click")).toBe("computer-use");
  });

  it("maps emergency stop onto a red safety state", () => {
    const topology = buildLiveTopology({
      ...liveInput(),
      computerStopped: true,
    });
    expect(
      (findNode(topology, "talos-core")?.metadata as { lifecycle?: string })
        .lifecycle,
    ).toBe("stopped");
    expect(STATUS_COLORS.stopped).toBe("#ef4444");
  });

  it("filters kinds while keeping sentinels and pruning edges", () => {
    const full = buildLiveTopology(liveInput());
    const filtered = applyKindFilter(
      full,
      new Set(["agent", "task", "tool", "policy"]),
    );
    const kinds = new Set(filtered.nodes.map((node) => node.kind));
    expect(kinds.has("browser")).toBe(false);
    expect(kinds.has("computer-use")).toBe(false);
    // Core + ledger + audit always stay for orientation.
    expect(kinds.has("system")).toBe(true);
    expect(kinds.has("evidence")).toBe(true);
    expect(kinds.has("audit")).toBe(true);
    for (const edge of filtered.edges) {
      expect(filtered.nodes.some((node) => node.id === edge.source)).toBe(true);
      expect(filtered.nodes.some((node) => node.id === edge.target)).toBe(true);
    }
  });

  it("maps node kinds onto inspector entity kinds", () => {
    const topology = buildLiveTopology(liveInput());
    const kinds = new Map<string, string>(
      topology.nodes.map((node) => [node.id, selectKindFor(node)]),
    );
    expect(kinds.get("a-1")).toBe("agent");
    expect(kinds.get("task-1")).toBe("task");
    expect(kinds.get("fs.read")).toBe("tool");
    expect(kinds.get("browser.click")).toBe("tool");
    expect(kinds.get("computer.click")).toBe("tool");
    expect(kinds.get("evidence-ledger")).toBe("evidence");
    expect(kinds.get("policy-gate")).toBe("system");
  });

  it("describes every node kind for tooltips and the inspector", () => {
    const topology = buildLiveTopology(liveInput());
    const described = new Map<string, string>(
      topology.nodes.map((node) => [node.id, describeNode(node)]),
    );
    expect(described.get("a-1")).toContain("running");
    expect(described.get("task-2")).toContain("waiting_policy");
    expect(described.get("fs.read")).toContain("registry");
    expect(described.get("policy-gate")).toContain("allow");
    expect(described.get("evidence-ledger")).toContain("3 records");
    expect(described.get("audit-sentinel")).toContain("watching");
    expect(described.get("talos-core")).toBe("Talos Core");
  });

  it("wires parent-child delegation edges", () => {
    const topology = buildLiveTopology({
      ...liveInput(),
      tasks: [
        {
          id: "parent-1",
          title: "Parent",
          status: "executing",
          parentTaskId: null,
        },
        {
          id: "child-1",
          title: "Child",
          status: "queued",
          parentTaskId: "parent-1",
        },
      ],
    });
    const delegation = topology.edges.find(
      (edge) => edge.id === "delegate-child-1",
    );
    expect(delegation).toBeDefined();
    expect(delegation?.source).toBe("parent-1");
    expect(delegation?.target).toBe("child-1");
    expect(delegation?.label).toBe("delegation");
  });

  it("detects recent task activity inside the window", () => {
    const now = Date.parse("2026-10-31T12:00:30.000Z");
    const feed = [
      { taskId: "task-1", at: "2026-10-31T12:00:10.000Z" },
      { taskId: "task-2", at: "2026-10-31T11:00:00.000Z" },
      { at: "2026-10-31T12:00:20.000Z" },
    ];
    expect(recentTaskIds(feed, now, 60_000)).toEqual(new Set(["task-1"]));
    expect(recentTaskIds(feed, now, 5_000)).toEqual(new Set());
  });

  it("counts browser and computer signals from the feed", () => {
    expect(
      countFeedSignals([
        { actor: "browser-use", message: "snapshot ok" },
        { actor: "computer-use", message: "EMERGENCY STOP engaged" },
        { actor: "runtime", message: "done" },
      ]),
    ).toEqual({ browser: 1, computer: 1 });
  });
});
