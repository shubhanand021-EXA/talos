import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { TaskQueueTable } from "./TaskQueueTable.js";
import { TaskQueueEntrySchema } from "@talos/contracts";

const STAMP = "2026-10-31T12:00:00.000Z";

const tasks = (["queued", "completed"] as const).map((status, index) =>
  TaskQueueEntrySchema.parse({
    id: `t-${index + 1}`,
    title: `Task ${index + 1}`,
    status,
    priority: index === 0 ? "critical" : "low",
    attempts: index,
    maxAttempts: 3,
    timeoutMs: 5000,
    scenario: "quick_success",
    createdAt: STAMP,
    updatedAt: STAMP,
  }),
);

describe("TaskQueueTable", () => {
  it("renders status badges, priorities, and attempts", () => {
    render(<TaskQueueTable tasks={tasks} />);
    expect(screen.getByText("Task 1")).toBeDefined();
    expect(screen.getByText("queued")).toBeDefined();
    expect(screen.getByText("critical")).toBeDefined();
    expect(screen.getByText("0/3")).toBeDefined();
    expect(screen.getByText("completed")).toBeDefined();
  });

  it("offers cancel and retry actions per row", () => {
    render(<TaskQueueTable tasks={tasks} />);
    expect(screen.getAllByText("Cancel")).toHaveLength(2);
    expect(screen.getAllByText("Retry")).toHaveLength(2);
  });
});
