import { describe, expect, it } from "vitest";
import {
  ApprovalRequestSchema,
  CompensationExecutionSchema,
  EffectClassSchema,
  PlanDocumentSchema,
  ReversibilitySchema,
  TaskModeSchema,
} from "./approval.js";
import { IpcEventName, IpcCommandName } from "./ipc.js";
import { TaskStatusSchema } from "./task.js";

const NOW = "2026-11-01T00:00:00.000Z";

function approval() {
  return {
    approvalId: "appr-task-1-1",
    taskId: "task-1",
    agentId: "agent-1",
    actionId: "req-1",
    summary: "Send this email to 43 recipients using the account work@example.com",
    target: "work@example.com",
    capability: "NetworkRequest",
    effectClass: "EXTERNAL_WRITE",
    reversibility: "irreversible",
    riskLevel: "high",
    policyReason: "require_human_review: high-risk non-reversible",
    requiredChecks: ["human_review"],
    payload: { to: "work@example.com" },
    payloadKind: "network",
    originalContent: null,
    proposedContent: null,
    diffFiles: [],
    compensation: [
      {
        order: 0,
        actionType: "manual_review",
        description: "Human verifies the rollback",
        payload: {},
        status: "pending",
      },
    ],
    costEstimate: { tokens: 240, estimatedUsd: 0.00048 },
    scope: "Grants req-1 one-shot permission; valid for this task only",
    expiresAt: NOW,
    requestedAt: NOW,
    status: "pending",
    decidedBy: null,
    denyReason: null,
  } as const;
}

describe("approval domain", () => {
  it("accepts all effect classes, modes, and reversibility", () => {
    for (const e of [
      "READ",
      "LOCAL_WRITE",
      "EXTERNAL_WRITE",
      "DESTRUCTIVE",
      "FINANCIAL",
      "PRIVILEGED",
    ] as const) {
      expect(EffectClassSchema.parse(e)).toBe(e);
    }
    for (const m of ["plan", "act", "auto"] as const) {
      expect(TaskModeSchema.parse(m)).toBe(m);
    }
    for (const r of ["reversible", "irreversible", "unknown"] as const) {
      expect(ReversibilitySchema.parse(r)).toBe(r);
    }
  });

  it("parses a full approval request", () => {
    expect(ApprovalRequestSchema.parse(approval())).toMatchObject({
      approvalId: "appr-task-1-1",
      status: "pending",
    });
  });

  it("parses a plan document", () => {
    const doc = {
      taskId: "task-1",
      mode: "plan",
      actions: [
        {
          approvalId: "appr-task-1-1",
          description: "Step 1",
          target: "/tmp/x",
          effect: "LOCAL_WRITE",
          reversibility: "reversible",
          risk: "medium",
          status: "pending",
        },
      ],
      createdAt: NOW,
      approvedCount: 0,
      pendingCount: 1,
    };
    expect(PlanDocumentSchema.parse(doc)).toMatchObject({ pendingCount: 1 });
  });

  it("parses a compensation execution with manual-review status", () => {
    const exec = {
      planId: "plan-1",
      taskId: "task-1",
      failureReason: "delete failed",
      steps: [],
      stepsCompleted: 0,
      status: "failed_needs_manual_review",
      startedAt: NOW,
      finishedAt: null,
    };
    expect(CompensationExecutionSchema.parse(exec).status).toBe(
      "failed_needs_manual_review",
    );
  });

  it("exposes HITL commands and events", () => {
    expect(IpcCommandName.grantApproval).toBe("grantApproval");
    expect(IpcCommandName.getPlanDocument).toBe("getPlanDocument");
    expect(IpcEventName.approvalRequested).toBe("approvalRequested");
    expect(IpcEventName.compensationExecuted).toBe("compensationExecuted");
  });

  it("tracks failed_needs_manual_review as a task status", () => {
    expect(TaskStatusSchema.parse("failed_needs_manual_review")).toBe(
      "failed_needs_manual_review",
    );
  });
});
