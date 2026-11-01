import { describe, expect, it } from "vitest";
import {
  AuditReportSummarySchema,
  AuditSeveritySchema,
  HeartbeatEventSchema,
  SceneEdgeSchema,
  SceneNodeSchema,
  SystemStatusSchema,
} from "./system.js";
import {
  IpcCommandName,
  IpcCommandNameSchema,
  IpcEventName,
  IpcEventNameSchema,
} from "./ipc.js";
import { isValidEnum, validateWith } from "./guards.js";
import { AgentRoleSchema } from "./agent.js";
import { TalosIdSchema } from "./identity.js";

const NOW = "2026-10-31T00:00:00.000Z";

describe("system domain", () => {
  it("parses system status and heartbeat", () => {
    const status = {
      version: "0.1.0",
      uptimeSeconds: 123,
      activeAgents: 2,
      activeTasks: 1,
      policyBrokerHealthy: true,
      evidenceLedgerHealthy: true,
      lastAuditAt: NOW,
    } as const;
    expect(SystemStatusSchema.parse(status)).toEqual(status);
    expect(() =>
      SystemStatusSchema.parse({ ...status, uptimeSeconds: -1 }),
    ).toThrow();

    const hb = {
      source: "policy-broker",
      timestamp: NOW,
      sequence: 7,
    } as const;
    expect(HeartbeatEventSchema.parse(hb)).toEqual(hb);
  });

  it("parses scene graph nodes and edges", () => {
    const node = {
      id: TalosIdSchema.parse("node-1"),
      label: "Coder",
      kind: "agent",
      status: "active",
      position: { x: 1, y: 2 },
    } as const;
    expect(SceneNodeSchema.parse(node)).toEqual(node);

    for (const kind of [
      "task",
      "tool",
      "browser",
      "computer-use",
      "policy",
      "evidence",
      "audit",
    ] as const) {
      expect(
        SceneNodeSchema.parse({ ...node, id: `node-${kind}`, kind }),
      ).toMatchObject({ kind });
    }

    const edge = {
      id: TalosIdSchema.parse("edge-1"),
      source: TalosIdSchema.parse("node-1"),
      target: TalosIdSchema.parse("node-2"),
      label: "spawns",
      state: "active",
    } as const;
    expect(SceneEdgeSchema.parse(edge)).toEqual(edge);
  });

  it("parses audit summaries", () => {
    const report = {
      id: TalosIdSchema.parse("audit-1"),
      severity: "warning",
      title: "Slow test suite",
      summary: "Suite exceeds budget",
      findingsCount: 3,
      passed: 8,
      failed: 1,
      createdAt: NOW,
    } as const;
    expect(AuditReportSummarySchema.parse(report)).toEqual(report);
    expect(() => AuditSeveritySchema.parse("fatal")).toThrow();
  });
});

describe("ipc constants", () => {
  it("command names are stable", () => {
    expect(Object.values(IpcCommandName)).toEqual([
      "getAppInfo",
      "getSystemStatus",
      "requestAction",
      "runDiagnostic",
      "verifyEvidenceLedger",
      "getAuditSummary",
      "submitTask",
      "cancelTask",
      "retryTask",
      "listTasks",
      "getTaskState",
      "listTools",
      "describeTool",
      "invokeToolDryRun",
      "invokeTool",
      "computerSafetyStatus",
      "computerEmergencyStop",
      "computerResetStop",
      "contextBudgetStatus",
      "contextCompact",
      "contextDiagnosticSnapshot",
      "queryToolCandidates",
      "listProviders",
      "listModels",
      "testProviderConnection",
      "getUsageSummary",
      "storeProviderKey",
      "deleteProviderKey",
      "previewContext",
      "getContextConfig",
      "updateContextConfig",
      "delegateTask",
      "getTaskTree",
      "cancelSubtree",
    ]);
    for (const name of Object.values(IpcCommandName)) {
      expect(IpcCommandNameSchema.parse(name)).toBe(name);
    }
    expect(() => IpcCommandNameSchema.parse("dropDatabase")).toThrow();
  });

  it("event names are stable", () => {
    expect(Object.values(IpcEventName)).toEqual([
      "heartbeat",
      "taskCreated",
      "taskUpdated",
      "agentSpawned",
      "agentUpdated",
      "policyEvaluated",
      "evidenceUpdated",
      "compensationRequired",
      "auditCompleted",
      "computerSafetyChanged",
      "contextCompacted",
      "llmRequestStarted",
      "llmRequestCompleted",
      "llmBudgetWarning",
      "contextBudgetExceeded",
      "browserWorkerCrashed",
      "agentDelegated",
      "childTaskSpawned",
      "childTaskCompleted",
      "childTaskFailed",
      "resultsMerged",
    ]);
    for (const name of Object.values(IpcEventName)) {
      expect(IpcEventNameSchema.parse(name)).toBe(name);
    }
    expect(() => IpcEventNameSchema.parse("allDone")).toThrow();
  });
});

describe("guards", () => {
  it("validateWith returns discriminated results", () => {
    const ok = validateWith(AgentRoleSchema, "coder");
    expect(ok).toEqual({ ok: true, value: "coder" });
    const bad = validateWith(AgentRoleSchema, "hacker");
    expect(bad.ok).toBe(false);
  });

  it("isValidEnum narrows correctly", () => {
    expect(isValidEnum(AgentRoleSchema, "coder")).toBe(true);
    expect(isValidEnum(AgentRoleSchema, "hacker")).toBe(false);
  });
});
