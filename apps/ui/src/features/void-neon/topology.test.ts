import { describe, expect, it } from "vitest";
import { AgentDescriptorSchema } from "@talos/contracts";
import {
  buildTopology,
  edgeColorFor,
  STATUS_COLORS,
  toTopologyStatus,
} from "./topology.js";

const STAMP = "2026-10-31T12:00:00.000Z";

function agent(
  id: string,
  status: "running" | "idle" | "waiting_policy" | "failed" | "completed",
) {
  return AgentDescriptorSchema.parse({
    id,
    displayName: id,
    role: "coder",
    status,
    sessionId: "session-local",
    capabilities: ["fs.read"],
    createdAt: STAMP,
    updatedAt: STAMP,
  });
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
});
