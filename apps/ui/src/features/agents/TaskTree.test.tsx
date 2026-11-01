import { describe, expect, it } from "vitest";
import type { TaskQueueEntry } from "@talos/contracts";
import { forestFromQueue } from "./TaskTree.js";

const NOW = "2026-10-31T00:00:00.000Z";

function entry(id: string, parentTaskId?: string): TaskQueueEntry {
  return {
    id: id as unknown as TaskQueueEntry["id"],
    title: id,
    status: "queued",
    priority: "normal",
    attempts: 0,
    maxAttempts: 3,
    timeoutMs: 5000,
    scenario: "quick_success",
    executionFidelity: "mock",
    createdAt: NOW,
    updatedAt: NOW,
    ...(parentTaskId
      ? {
          parentTaskId:
            parentTaskId as unknown as TaskQueueEntry["parentTaskId"],
        }
      : {}),
  };
}

describe("task forest", () => {
  it("nests children under parents", () => {
    const roots = forestFromQueue([
      entry("parent-1"),
      entry("child-1", "parent-1"),
    ]);
    expect(roots).toHaveLength(1);
    expect(roots[0]?.task.id).toBe("parent-1");
    expect(roots[0]?.children).toHaveLength(1);
    expect(roots[0]?.children[0]?.task.id).toBe("child-1");
  });

  it("keeps orphan tasks as roots", () => {
    const roots = forestFromQueue([entry("lonely")]);
    expect(roots).toHaveLength(1);
  });
});
