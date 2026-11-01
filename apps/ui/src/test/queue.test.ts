import { describe, expect, it } from "vitest";
import { TaskQueueEntrySchema } from "@talos/contracts";
import { useAppStore } from "../stores/appStore.js";

const STAMP = "2026-10-31T12:00:00.000Z";

function entry(id: string, status: string, priority = "high") {
  return TaskQueueEntrySchema.parse({
    id,
    title: `Task ${id}`,
    status,
    priority,
    attempts: 1,
    maxAttempts: 3,
    timeoutMs: 5000,
    scenario: "quick_success",
    executionFidelity: "mock",
    createdAt: STAMP,
    updatedAt: STAMP,
  });
}

describe("durable queue store", () => {
  it("upserts live tasks by id", () => {
    useAppStore.getState().setLiveTasks([]);
    useAppStore.getState().upsertLiveTask(entry("t-1", "queued"));
    expect(useAppStore.getState().liveTasks).toHaveLength(1);
    useAppStore
      .getState()
      .upsertLiveTask(entry("t-1", "completed", "critical"));
    const tasks = useAppStore.getState().liveTasks;
    expect(tasks).toHaveLength(1);
    expect(tasks[0]?.status).toBe("completed");
    expect(tasks[0]?.priority).toBe("critical");
  });

  it("validates queue entries and rejects bad statuses", () => {
    expect(() =>
      TaskQueueEntrySchema.parse({
        ...entry("t-2", "completed"),
        status: "done",
      }),
    ).toThrow();
  });
});
