import { describe, expect, it } from "vitest";
import {
  ArtifactRefSchema,
  DelegateTaskInputSchema,
  DelegationRequestSchema,
  DelegationResultSchema,
  PlanPreviewViewSchema,
  PlanningConfigViewSchema,
  PlanningMethodSchema,
  PlanStepViewSchema,
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
    executionFidelity: "mock",
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
      executionFidelity: "live",
    };
    expect(ArtifactRefSchema.parse(artifact)).toMatchObject({
      kind: "code_patch",
      executionFidelity: "live",
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
      executionFidelity: "mock",
    };
    expect(DelegationResultSchema.parse(result)).toMatchObject({
      status: "completed",
      executionFidelity: "mock",
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

  it("parses plan preview views", () => {
    const step = {
      stepId: "a",
      role: "researcher",
      goal: "gather context",
      dependsOn: [],
      toolsNeeded: ["fs.read"],
      requiresApproval: false,
      estimatedTokens: 100,
    };
    expect(PlanStepViewSchema.parse(step)).toMatchObject({ stepId: "a" });
    expect(() =>
      PlanStepViewSchema.parse({ ...step, role: "hacker" }),
    ).toThrow();
    const preview = {
      parentTaskId: "parent-1",
      method: "llm",
      steps: [step],
      levels: [["a"]],
      requiresApproval: false,
      valid: true,
    };
    expect(PlanPreviewViewSchema.parse(preview)).toMatchObject({
      method: "llm",
    });
    expect(PlanningMethodSchema.parse("keyword")).toBe("keyword");
    expect(() => PlanningMethodSchema.parse("telepathy")).toThrow();
    expect(
      PlanningConfigViewSchema.parse({
        model: "gpt-4o-mini",
        timeoutMs: 30_000,
        fallbackEnabled: true,
      }),
    ).toMatchObject({ model: "gpt-4o-mini" });
    expect(() =>
      PlanningConfigViewSchema.parse({
        model: "gpt-4o-mini",
        timeoutMs: 50,
        fallbackEnabled: true,
      }),
    ).toThrow();
  });
});
