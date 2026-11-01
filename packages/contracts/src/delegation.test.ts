import { describe, expect, it } from "vitest";
import {
  ArtifactRefSchema,
  DelegateTaskInputSchema,
  DelegationRequestSchema,
  DelegationResultSchema,
  SpecialistRoleSchema,
} from "./delegation.js";
import { TaskQueueEntrySchema, TaskTreeNodeSchema } from "./task.js";

const NOW = "2026-10-31T00:00:00.000Z";

function queueEntry(overrides: Record<string, unknown> = {}) {
  return {
    id: "task-1",
    title: "parent",
    status: "queued",
    priority: "normal",
    attempts: 0,
    maxAttempts: 3,
    timeoutMs: 5000,
    scenario: "quick_success",
    createdAt: NOW,
    updatedAt: NOW,
    ...overrides,
  };
}

describe("delegation contracts", () => {
  it("parses specialist roles and artifacts", () => {
    expect(SpecialistRoleSchema.parse("coder")).toBe("coder");
    expect(() => SpecialistRoleSchema.parse("hacker")).toThrow();
    const artifact = {
      id: "c1-artifact-1",
      kind: "code_patch",
      summary: "coder slice ready",
    };
    expect(ArtifactRefSchema.parse(artifact)).toMatchObject({
      kind: "code_patch",
    });
  });

  it("parses delegation request and result", () => {
    const request = {
      parentTaskId: "parent-1",
      childTaskId: "parent-1-child-1-coder",
      agentRole: "coder",
      scope: "code slice",
      toolAllowlist: ["fs.read"],
      contextBudget: 4000,
      deadlineMs: 30_000,
      depth: 2,
    };
    expect(DelegationRequestSchema.parse(request)).toMatchObject({
      agentRole: "coder",
      depth: 2,
    });
    const result = {
      childTaskId: "parent-1-child-1-coder",
      status: "completed",
      artifacts: [],
      evidenceIds: [],
      limitations: [],
      remainingRisks: [],
    };
    expect(DelegationResultSchema.parse(result)).toMatchObject({
      status: "completed",
    });
  });

  it("queue entries carry optional parent task ids", () => {
    expect(TaskQueueEntrySchema.parse(queueEntry())).toMatchObject({
      id: "task-1",
    });
    expect(
      TaskQueueEntrySchema.parse(queueEntry({ parentTaskId: "parent-1" })),
    ).toMatchObject({ parentTaskId: "parent-1" });
  });

  it("parses nested task trees", () => {
    const tree = {
      task: queueEntry({ id: "parent-1" }),
      children: [
        {
          task: queueEntry({ id: "child-1", parentTaskId: "parent-1" }),
          children: [],
        },
      ],
    };
    expect(TaskTreeNodeSchema.parse(tree)).toMatchObject({
      task: { id: "parent-1" },
    });
  });

  it("parses delegate input", () => {
    expect(
      DelegateTaskInputSchema.parse({
        parentTaskId: "parent-1",
        goal: "fix code",
      }),
    ).toMatchObject({ parentTaskId: "parent-1" });
  });
});
