import {
  TalosIdSchema,
  type AgentDescriptor,
  type SceneEdge,
  type SceneNode,
  type TalosId,
} from "@talos/contracts";
import type { Topology, TopologyStatus } from "./types.js";

/** Neon color per lifecycle status. Single source for materials + HUD dots. */
export const STATUS_COLORS: Record<TopologyStatus, string> = {
  idle: "#64748b",
  running: "#22d3ee",
  waiting_policy: "#fbbf24",
  failed: "#fb7185",
  completed: "#34d399",
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
        lifecycle: toTopologyStatus(agent.status),
      },
    });
    edges.push({
      id: TalosIdSchema.parse(`core-link-${agent.id}`),
      source: coreId,
      target: nodeId,
      label: agent.role,
      state: "active",
    });
  });

  return { nodes, edges, coreId };
}

/** Node lookup by id (used by HUD + selection). */
export function findNode(
  topology: Topology,
  id: string,
): SceneNode | undefined {
  return topology.nodes.find((node) => node.id === id);
}

/** Lifecycle color for an edge, derived from its target node metadata. */
export function edgeColorFor(topology: Topology, edge: SceneEdge): string {
  const target = findNode(topology, edge.target);
  const lifecycle = (
    target?.metadata as { lifecycle?: TopologyStatus } | undefined
  )?.lifecycle;
  return (
    (lifecycle !== undefined ? STATUS_COLORS[lifecycle] : undefined) ??
    "#22d3ee"
  );
}
