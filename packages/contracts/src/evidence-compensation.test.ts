import { describe, expect, expectTypeOf, it } from "vitest";
import {
  ClaimRecordSchema,
  EvidenceKindSchema,
  EvidenceRecordSchema,
  EvidenceStatusSchema,
} from "./evidence.js";
import type { EvidenceRecord } from "./evidence.js";
import {
  CompensationPlanSchema,
  CompensationStepSchema,
  CompensationStrategySchema,
} from "./compensation.js";
import {
  AgentIdSchema,
  CompensationPlanIdSchema,
  EvidenceIdSchema,
  SessionIdSchema,
  TalosIdSchema,
  TaskIdSchema,
} from "./identity.js";

const NOW = "2026-10-31T00:00:00.000Z";

describe("evidence domain", () => {
  it("accepts all kinds and statuses", () => {
    for (const k of [
      "log",
      "file_hash",
      "screenshot",
      "terminal_output",
      "command_result",
      "state_snapshot",
      "test_result",
      "metric",
    ] as const) {
      expect(EvidenceKindSchema.parse(k)).toBe(k);
    }
    for (const s of [
      "pending",
      "verified",
      "rejected",
      "conflicting",
    ] as const) {
      expect(EvidenceStatusSchema.parse(s)).toBe(s);
    }
  });

  it("rejects invalid kind/status", () => {
    expect(() => EvidenceKindSchema.parse("rumor")).toThrow();
    expect(() => EvidenceStatusSchema.parse("maybe")).toThrow();
  });

  it("payload stays unknown", () => {
    expectTypeOf<EvidenceRecord["payload"]>().toEqualTypeOf<unknown>();
  });

  it("parses evidence and claim records", () => {
    const evidence: EvidenceRecord = {
      id: EvidenceIdSchema.parse("ev-1"),
      taskId: TaskIdSchema.parse("task-1"),
      agentId: AgentIdSchema.parse("agent-1"),
      sessionId: SessionIdSchema.parse("session-1"),
      kind: "command_result",
      status: "verified",
      label: "pnpm test",
      payload: { exitCode: 0 },
      createdAt: NOW,
    };
    expect(EvidenceRecordSchema.parse(evidence)).toEqual(evidence);

    const claim = {
      id: TalosIdSchema.parse("claim-1"),
      statement: "Tests pass on clean machine",
      evidenceIds: [EvidenceIdSchema.parse("ev-1")],
      createdByAgentId: AgentIdSchema.parse("agent-1"),
      createdAt: NOW,
    } as const;
    expect(ClaimRecordSchema.parse(claim)).toEqual(claim);
    expect(() =>
      ClaimRecordSchema.parse({ ...claim, evidenceIds: [] }),
    ).toThrow();
  });
});

describe("compensation domain", () => {
  it("accepts all strategies", () => {
    for (const s of [
      "backup_restore",
      "snapshot_revert",
      "rollback_command",
      "manual_review",
    ] as const) {
      expect(CompensationStrategySchema.parse(s)).toBe(s);
    }
    expect(() => CompensationStrategySchema.parse("yolo")).toThrow();
  });

  it("parses steps and plans, requires at least one step", () => {
    const step = {
      id: TalosIdSchema.parse("step-1"),
      strategy: "rollback_command",
      description: "Revert file write",
      command: "git checkout -- out.js",
    } as const;
    expect(CompensationStepSchema.parse(step)).toEqual(step);

    const plan = {
      id: CompensationPlanIdSchema.parse("plan-1"),
      reason: "Write may clobber user file",
      steps: [step],
      createdAt: NOW,
    } as const;
    expect(CompensationPlanSchema.parse(plan)).toEqual(plan);
    expect(() =>
      CompensationPlanSchema.parse({ ...plan, steps: [] }),
    ).toThrow();
  });
});
