import {
  TalosIdSchema,
  type AgentDescriptor,
  type SceneEdge,
  type SceneEdgeState,
  type SceneNode,
  type SceneNodeKind,
  type TalosId,
} from "@talos/contracts";
import type { Topology, TopologyStatus } from "./types.js";

/** Neon color per lifecycle status. Single source for materials + HUD dots. */
export const STATUS_COLORS: Record<TopologyStatus, string> = {
  idle: "#64748b",
  running: "#22d3ee",
  queued: "#475569",
  waiting_policy: "#fbbf24",
  denied: "#fb7185",
  failed: "#fb7185",
  completed: "#34d399",
  canceled: "#6b7280",
  stopped: "#ef4444",
};

/** Edge color per edge state (flow semantics, not node color). */
export const EDGE_STATE_COLORS: Record<SceneEdgeState, string> = {
  active: "#22d3ee",
  pending: "#fbbf24",
  blocked: "#64748b",
  completed: "#34d399",
  failed: "#fb7185",
};

/** Map an agent status onto a scene status (unknown → idle, never throws). */
export function toTopologyStatus(
  status: AgentDescriptor["status"],
): TopologyStatus {
  switch (status) {
    case "running":
    case "starting":
      return "running";
    case "waiting_policy":
      return "waiting_policy";
    case "failed":
      return "failed";
    case "completed":
    case "terminated":
      return "completed";
    default:
      return "idle";
  }
}

/** Map a task queue status onto a scene status (unknown → idle). */
export function toTaskTopologyStatus(status: string): TopologyStatus {
  switch (status) {
    case "queued":
      return "queued";
    case "planning":
    case "executing":
    case "canceling":
      return "running";
    case "waiting_policy":
    case "waiting_evidence":
    case "waiting_compensation":
      return "waiting_policy";
    case "denied":
      return "denied";
    case "failed":
    case "timed_out":
      return "failed";
    case "completed":
      return "completed";
    case "canceled":
      return "canceled";
    default:
      return "idle";
  }
}

/** Lifecycle color for an edge from its own state (flow semantics). */
export function edgeColorFor(_topology: Topology, edge: SceneEdge): string {
  return EDGE_STATE_COLORS[edge.state];
}

/** Edge state for an agent assignment link. */
function agentEdgeState(lifecycle: TopologyStatus): SceneEdge["state"] {
  switch (lifecycle) {
    case "failed":
    case "denied":
    case "stopped":
      return "failed";
    case "waiting_policy":
      return "pending";
    case "completed":
      return "completed";
    default:
      return "active";
  }
}

/** Edge state for a task lifecycle link. */
function taskEdgeState(lifecycle: TopologyStatus): SceneEdge["state"] {
  switch (lifecycle) {
    case "failed":
    case "denied":
      return "failed";
    case "waiting_policy":
    case "queued":
      return "pending";
    case "completed":
      return "completed";
    case "canceled":
      return "blocked";
    default:
      return "active";
  }
}

/**
 * Deterministic topology: core at origin, agents on a golden-angle spiral.
 * Pure function of agent order — same snapshot on every machine.
 */
export function buildTopology(agents: AgentDescriptor[]): Topology {
  const coreId: TalosId = TalosIdSchema.parse("talos-core");
  const nodes: SceneNode[] = [
    {
      id: coreId,
      label: "Talos Core",
      kind: "system",
      status: "active",
      position: { x: 0, y: 0, z: 0 },
      metadata: { role: "orchestrator" },
    },
  ];
  const edges: SceneEdge[] = [];
  const golden = Math.PI * (3 - Math.sqrt(5));

  agents.forEach((agent, index) => {
    const angle = index * golden;
    const radius = 3.2 + index * 0.55;
    const nodeId: TalosId = TalosIdSchema.parse(agent.id);
    const lifecycle = toTopologyStatus(agent.status);
    nodes.push({
      id: nodeId,
      label: agent.displayName,
      kind: "agent",
      status: "active",
      position: {
        x: Math.cos(angle) * radius,
        y: Math.sin(index * 1.1) * 0.8,
        z: Math.sin(angle) * radius,
      },
      metadata: {
        role: agent.role,
        lifecycle,
      },
    });
    edges.push({
      id: TalosIdSchema.parse(`core-link-${agent.id}`),
      source: coreId,
      target: nodeId,
      label: agent.role,
      state: agentEdgeState(lifecycle),
    });
  });

  return { nodes, edges, coreId };
}

/** Minimal task row the live topology needs (queue entries + mock records). */
export interface TopologyTask {
  id: string;
  title: string;
  status: string;
  parentTaskId?: string | null;
}

/** Input for the live operational topology (all plain data, no stores). */
export interface LiveTopologyInput {
  agents: AgentDescriptor[];
  tasks: TopologyTask[];
  /** Registered tool ids (e.g. `ToolIdSchema.options`). */
  toolIds: string[];
  /** Emergency stop latched: core goes red, flows freeze. */
  computerStopped: boolean;
  /** Latest policy verdict, if any. */
  policyDecision: string | null;
  /** Evidence rows recorded (drives the ledger node). */
  evidenceCount: number;
}

/** Render caps: hard bounds so frame cost stays flat as queues grow. */
export const TOPOLOGY_CAPS = {
  tasks: 16,
  tools: 32,
} as const;

/** Classify a tool id into its scene kind. */
export function toolKindFor(
  toolId: string,
): "tool" | "browser" | "computer-use" {
  if (toolId.startsWith("browser.")) {
    return "browser";
  }
  if (toolId.startsWith("computer.")) {
    return "computer-use";
  }
  return "tool";
}

/**
 * Live operational topology: core + agents + tasks + tools + policy gate +
 * evidence ledger + audit sentinel. Deterministic in input order; rings are
 * fixed per kind so nodes never jump between frames.
 */
export function buildLiveTopology(input: LiveTopologyInput): Topology {
  const coreId: TalosId = TalosIdSchema.parse("talos-core");
  const coreLifecycle: TopologyStatus = input.computerStopped
    ? "stopped"
    : "running";
  const nodes: SceneNode[] = [
    {
      id: coreId,
      label: "Talos Core",
      kind: "system",
      status: "active",
      position: { x: 0, y: 0, z: 0 },
      metadata: { role: "orchestrator", lifecycle: coreLifecycle },
    },
  ];
  const edges: SceneEdge[] = [];
  const golden = Math.PI * (3 - Math.sqrt(5));

  // Agents: inner ring (golden spiral, radius ~3.2).
  const agentLifecycle = new Map<string, TopologyStatus>();
  input.agents.forEach((agent, index) => {
    const angle = index * golden;
    const radius = 3.2 + index * 0.55;
    const lifecycle = toTopologyStatus(agent.status);
    agentLifecycle.set(agent.id, lifecycle);
    const nodeId: TalosId = TalosIdSchema.parse(agent.id);
    nodes.push({
      id: nodeId,
      label: agent.displayName,
      kind: "agent",
      status: "active",
      position: {
        x: Math.cos(angle) * radius,
        y: Math.sin(index * 1.1) * 0.8,
        z: Math.sin(angle) * radius,
      },
      metadata: { role: agent.role, lifecycle, taskId: agent.taskId ?? null },
    });
    edges.push({
      id: TalosIdSchema.parse(`core-link-${agent.id}`),
      source: coreId,
      target: nodeId,
      label: "assignment",
      state: agentEdgeState(lifecycle),
    });
  });

  // Tasks: middle ring (radius ~5.6), capped for flat frame cost.
  const agentByTaskId = new Map<string, string>();
  for (const agent of input.agents) {
    if (agent.taskId !== undefined) {
      agentByTaskId.set(agent.taskId, agent.id);
    }
  }
  input.tasks.slice(0, TOPOLOGY_CAPS.tasks).forEach((task, index) => {
    const angle = index * golden + 1.1;
    const radius = 5.6 + (index % 4) * 0.35;
    const lifecycle = toTaskTopologyStatus(task.status);
    const nodeId: TalosId = TalosIdSchema.parse(task.id);
    nodes.push({
      id: nodeId,
      label: task.title,
      kind: "task",
      status:
        lifecycle === "failed" || lifecycle === "denied" ? "error" : "active",
      position: {
        x: Math.cos(angle) * radius,
        y: 1.4 - (index % 3) * 0.9,
        z: Math.sin(angle) * radius,
      },
      metadata: {
        lifecycle,
        queueStatus: task.status,
        parentTaskId: task.parentTaskId ?? null,
      },
    });
    // Delegation edge: child → parent pulses when a Supervisor spawned it.
    if (task.parentTaskId !== undefined && task.parentTaskId !== null) {
      try {
        edges.push({
          id: TalosIdSchema.parse(`delegate-${task.id}`),
          source: TalosIdSchema.parse(task.parentTaskId),
          target: nodeId,
          label: "delegation",
          state: taskEdgeState(lifecycle),
        });
      } catch {
        // Unknown parent ids (mock fixtures) skip the delegation edge.
      }
    }
    // Assignment: owning agent when known, else the core queue.
    const owner = agentByTaskId.get(task.id);
    edges.push({
      id: TalosIdSchema.parse(`assign-${task.id}`),
      source: owner !== undefined ? TalosIdSchema.parse(owner) : coreId,
      target: nodeId,
      label: "task assignment",
      state: taskEdgeState(lifecycle),
    });
    // Policy gate flow for held tasks.
    if (
      task.status === "waiting_policy" ||
      task.status === "waiting_evidence" ||
      task.status === "waiting_compensation"
    ) {
      edges.push({
        id: TalosIdSchema.parse(`gate-${task.id}`),
        source: nodeId,
        target: TalosIdSchema.parse("policy-gate"),
        label: "policy gate",
        state: "pending",
      });
    }
    // Evidence flow for finished tasks.
    if (
      task.status === "completed" ||
      task.status === "failed" ||
      task.status === "timed_out"
    ) {
      edges.push({
        id: TalosIdSchema.parse(`ledger-${task.id}`),
        source: nodeId,
        target: TalosIdSchema.parse("evidence-ledger"),
        label: "evidence recording",
        state: task.status === "completed" ? "completed" : "failed",
      });
    }
  });

  // Tools: outer ring (radius ~8), sectors per family, capped.
  const tools = [...input.toolIds].sort().slice(0, TOPOLOGY_CAPS.tools);
  tools.forEach((toolId, index) => {
    const kind = toolKindFor(toolId);
    const sector = kind === "browser" ? 0 : kind === "computer-use" ? 2.1 : 4.2;
    const angle = sector + (index % 8) * 0.24;
    const radius = 8 + Math.floor(index / 8) * 0.6;
    const nodeId: TalosId = TalosIdSchema.parse(toolId);
    nodes.push({
      id: nodeId,
      label: toolId,
      kind,
      status: "active",
      position: {
        x: Math.cos(angle) * radius,
        y: -1.2 + (index % 3) * 0.8,
        z: Math.sin(angle) * radius,
      },
      metadata: { lifecycle: "idle", family: kind },
    });
    edges.push({
      id: TalosIdSchema.parse(`invoke-${toolId}`),
      source: coreId,
      target: nodeId,
      label: kind === "tool" ? "tool invocation" : "browser/computer action",
      state: "active",
    });
  });

  // Policy gate sentinel.
  const policyLifecycle: TopologyStatus =
    input.policyDecision === "deny"
      ? "denied"
      : input.policyDecision === "require_human_review"
        ? "waiting_policy"
        : input.policyDecision === null
          ? "idle"
          : "completed";
  nodes.push({
    id: TalosIdSchema.parse("policy-gate"),
    label: "Policy Gate",
    kind: "policy",
    status: policyLifecycle === "denied" ? "error" : "active",
    position: { x: 0, y: 3.4, z: -4.2 },
    metadata: { lifecycle: policyLifecycle, decision: input.policyDecision },
  });

  // Evidence ledger sentinel.
  nodes.push({
    id: TalosIdSchema.parse("evidence-ledger"),
    label: "Evidence Ledger",
    kind: "evidence",
    status: "active",
    position: { x: 0, y: -3.2, z: -4.2 },
    metadata: {
      lifecycle: input.evidenceCount > 0 ? "completed" : "idle",
      records: input.evidenceCount,
    },
  });

  // Audit sentinel.
  nodes.push({
    id: TalosIdSchema.parse("audit-sentinel"),
    label: "Audit Sentinel",
    kind: "audit",
    status: "active",
    position: { x: 0, y: 0.2, z: 4.8 },
    metadata: { lifecycle: "idle" },
  });

  return { nodes, edges, coreId };
}

/** Filter groups surfaced in the viewport controls. */
export const KIND_FILTER_GROUPS = [
  { kind: "agent", label: "Agents" },
  { kind: "task", label: "Tasks" },
  { kind: "tool", label: "Tools" },
  { kind: "browser", label: "Browser" },
  { kind: "computer-use", label: "Computer-use" },
  { kind: "policy", label: "Policy" },
] as const;

/** Filter a topology to the visible kinds (core/evidence/audit always stay). */
export function applyKindFilter(
  topology: Topology,
  visible: ReadonlySet<SceneNodeKind>,
): Topology {
  const alwaysVisible: ReadonlySet<string> = new Set([
    "system",
    "evidence",
    "audit",
  ]);
  const kept = new Set<string>();
  const nodes = topology.nodes.filter((node) => {
    const show = visible.has(node.kind) || alwaysVisible.has(node.kind);
    if (show) {
      kept.add(node.id);
    }
    return show;
  });
  const edges = topology.edges.filter(
    (edge) => kept.has(edge.source) && kept.has(edge.target),
  );
  return { nodes, edges, coreId: topology.coreId };
}

/** Human description for a node (HUD tooltip + inspector detail). */
export function describeNode(node: SceneNode): string {
  const meta = node.metadata as
    | {
        role?: string;
        lifecycle?: string;
        queueStatus?: string;
        decision?: string | null;
        records?: number;
        family?: string;
      }
    | undefined;
  switch (node.kind) {
    case "agent": {
      const role = meta?.role ?? "agent";
      const lifecycle = meta?.lifecycle;
      return lifecycle === undefined ? role : `${role} · ${lifecycle}`;
    }
    case "task": {
      const status = meta?.queueStatus ?? meta?.lifecycle ?? "unknown";
      return `task · ${status}`;
    }
    case "tool":
    case "browser":
    case "computer-use":
      return `${meta?.family ?? node.kind} tool · registry`;
    case "policy":
      return meta?.decision == null
        ? "policy gate · no verdict yet"
        : `policy gate · ${meta.decision}`;
    case "evidence":
      return `ledger · ${meta?.records ?? 0} records`;
    case "audit":
      return "audit sentinel · watching";
    default:
      return node.label;
  }
}

/** Inspector entity kind for a scene node. */
export function selectKindFor(
  node: SceneNode,
): "agent" | "task" | "tool" | "evidence" | "system" {
  switch (node.kind) {
    case "agent":
      return "agent";
    case "task":
      return "task";
    case "tool":
    case "browser":
    case "computer-use":
      return "tool";
    case "evidence":
      return "evidence";
    default:
      return "system";
  }
}

/** Task ids with activity inside the window (feed `at` within `windowMs` of `now`). */
export function recentTaskIds(
  feed: Array<{ taskId?: string; at: string }>,
  now: number,
  windowMs: number,
): Set<string> {
  const ids = new Set<string>();
  for (const event of feed) {
    if (event.taskId === undefined) {
      continue;
    }
    const at = Date.parse(event.at);
    if (!Number.isNaN(at) && now - at >= 0 && now - at <= windowMs) {
      ids.add(event.taskId);
    }
  }
  return ids;
}

/** Feed-derived action counts for the HUD (actor/message substring match). */
export function countFeedSignals(
  feed: Array<{ actor: string; message: string }>,
): { browser: number; computer: number } {
  let browser = 0;
  let computer = 0;
  for (const event of feed) {
    const haystack = `${event.actor} ${event.message}`;
    if (/browser/i.test(haystack)) {
      browser += 1;
    }
    if (/computer/i.test(haystack)) {
      computer += 1;
    }
  }
  return { browser, computer };
}

/** Node lookup by id (used by HUD + selection). */
export function findNode(
  topology: Topology,
  id: string,
): SceneNode | undefined {
  return topology.nodes.find((node) => node.id === id);
}
