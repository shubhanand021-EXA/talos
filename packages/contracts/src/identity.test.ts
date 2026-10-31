import { describe, expect, it } from "vitest";
import {
  AgentIdSchema,
  CompensationPlanIdSchema,
  EvidenceIdSchema,
  IsoDateTimeSchema,
  SessionIdSchema,
  TaskIdSchema,
  parseAgentId,
} from "./identity.js";

const NOW = "2026-10-31T00:00:00.000Z";

describe("identity", () => {
  it("accepts valid ISO datetimes", () => {
    expect(IsoDateTimeSchema.parse(NOW)).toBe(NOW);
  });

  it("rejects non-ISO datetimes", () => {
    expect(() => IsoDateTimeSchema.parse("not-a-date")).toThrow();
    expect(() => IsoDateTimeSchema.parse("2026-13-99")).toThrow();
    expect(() => IsoDateTimeSchema.parse(123)).toThrow();
  });

  it("brands domain ids", () => {
    expect(AgentIdSchema.parse("agent-1")).toBe("agent-1");
    expect(TaskIdSchema.parse("task-1")).toBe("task-1");
    expect(SessionIdSchema.parse("session-1")).toBe("session-1");
    expect(EvidenceIdSchema.parse("ev-1")).toBe("ev-1");
    expect(CompensationPlanIdSchema.parse("plan-1")).toBe("plan-1");
  });

  it("rejects empty ids", () => {
    expect(() => AgentIdSchema.parse("")).toThrow();
    expect(() => TaskIdSchema.parse("")).toThrow();
  });

  it("parse helpers throw on invalid input", () => {
    expect(parseAgentId("agent-9")).toBe("agent-9");
    expect(() => parseAgentId("")).toThrow();
  });
});
