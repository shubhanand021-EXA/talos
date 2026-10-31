import { describe, expect, expectTypeOf, it } from "vitest";
import {
  AgentDescriptorSchema,
  AgentRoleSchema,
  AgentStatusSchema,
} from "./agent.js";
import type { AgentDescriptor } from "./agent.js";
import {
  TaskPrioritySchema,
  TaskRecordSchema,
  TaskStatusSchema,
} from "./task.js";
import {
  ActionRequestSchema,
  PolicyDecisionSchema,
  PolicyEvaluationSchema,
  RiskLevelSchema,
} from "./policy.js";
import type { ActionRequest } from "./policy.js";
import {
  AgentIdSchema,
  SessionIdSchema,
  TalosIdSchema,
  TaskIdSchema,
} from "./identity.js";

const NOW = "2026-10-31T00:00:00.000Z";

function ids() {
  return {
    agentId: AgentIdSchema.parse("agent-1"),
    taskId: TaskIdSchema.parse("task-1"),
    sessionId: SessionIdSchema.parse("session-1"),
    requestId: TalosIdSchema.parse("req-1"),
  };
}

describe("agent domain", () => {
  it("accepts all roles and statuses", () => {
    for (const r of [
      "orchestrator",
      "coder",
      "browser",
      "computer-use",
      "auditor",
      "researcher",
      "custom",
    ] as const) {
      expect(AgentRoleSchema.parse(r)).toBe(r);
    }
    for (const s of [
      "idle",
      "starting",
      "running",
      "waiting_policy",
      "failed",
      "terminated",
      "completed",
    ] as const) {
      expect(AgentStatusSchema.parse(s)).toBe(s);
    }
  });

  it("rejects unknown role/status", () => {
    expect(() => AgentRoleSchema.parse("hacker")).toThrow();
    expect(() => AgentStatusSchema.parse("sleeping")).toThrow();
  });

  it("parses a valid descriptor, rejects strict violations", () => {
    const { agentId, sessionId, taskId } = ids();
    const descriptor: AgentDescriptor = {
      id: agentId,
      displayName: "Coder 1",
      role: "coder",
      status: "running",
      sessionId,
      taskId,
      capabilities: ["fs.read", "fs.write"],
      createdAt: NOW,
      updatedAt: NOW,
    };
    expect(AgentDescriptorSchema.parse(descriptor)).toEqual(descriptor);
    expect(() =>
      AgentDescriptorSchema.parse({ ...descriptor, extra: 1 }),
    ).toThrow();
    expect(() =>
      AgentDescriptorSchema.parse({ ...descriptor, displayName: "" }),
    ).toThrow();
  });
});

describe("task domain", () => {
  it("accepts all statuses and priorities", () => {
    for (const s of [
      "queued",
      "planning",
      "executing",
      "waiting_policy",
      "waiting_evidence",
      "waiting_compensation",
      "canceling",
      "canceled",
      "completed",
      "failed",
      "timed_out",
    ] as const) {
      expect(TaskStatusSchema.parse(s)).toBe(s);
    }
    for (const p of ["low", "normal", "high", "critical"] as const) {
      expect(TaskPrioritySchema.parse(p)).toBe(p);
    }
  });

  it("rejects invalid enums", () => {
    expect(() => TaskStatusSchema.parse("done")).toThrow();
    expect(() => TaskPrioritySchema.parse("urgent")).toThrow();
  });

  it("parses a valid task record", () => {
    const { taskId, sessionId, agentId } = ids();
    const record = {
      id: taskId,
      sessionId,
      title: "Implement contracts",
      status: "executing",
      priority: "high",
      agentId,
      createdAt: NOW,
      updatedAt: NOW,
    } as const;
    expect(TaskRecordSchema.parse(record)).toEqual(record);
  });
});

describe("policy domain", () => {
  it("accepts all risk levels and decisions", () => {
    for (const r of ["low", "medium", "high", "irreversible"] as const) {
      expect(RiskLevelSchema.parse(r)).toBe(r);
    }
    for (const d of [
      "allow",
      "deny",
      "require_evidence",
      "require_compensation",
      "require_human_review",
    ] as const) {
      expect(PolicyDecisionSchema.parse(d)).toBe(d);
    }
  });

  it("keeps payload as unknown (no any leakage)", () => {
    expectTypeOf<ActionRequest["payload"]>().toEqualTypeOf<unknown>();
  });

  it("parses action request and evaluation", () => {
    const { agentId, taskId, requestId } = ids();
    const request: ActionRequest = {
      id: requestId,
      agentId,
      taskId,
      capability: "fs.write",
      intent: "Write compiled output",
      target: "/tmp/talos/out.js",
      payload: { bytes: 42 },
      riskLevel: "medium",
      reversible: true,
      requestedAt: NOW,
    };
    expect(ActionRequestSchema.parse(request)).toEqual(request);
    const evaluation = {
      id: TalosIdSchema.parse("eval-1"),
      actionRequestId: requestId,
      decision: "allow",
      reason: "Low blast radius, reversible",
      policyVersion: "v1",
      evaluatedAt: NOW,
    } as const;
    expect(PolicyEvaluationSchema.parse(evaluation)).toEqual(evaluation);
    expect(() =>
      ActionRequestSchema.parse({ ...request, riskLevel: "unknown" }),
    ).toThrow();
  });
});
