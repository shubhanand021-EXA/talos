import { describe, expect, it } from "vitest";
import {
  ComputerActionRequestSchema,
  ComputerActionSchema,
  ComputerReceiptSchema,
  ComputerTargetSchema,
} from "./computer.js";
import { TaskIdSchema } from "./identity.js";

const NOW = "2026-10-31T00:00:00.000Z";

describe("computer domain", () => {
  it("accepts all nine computer actions", () => {
    for (const action of [
      "move_mouse",
      "click",
      "double_click",
      "drag",
      "type_text",
      "hotkey",
      "scroll",
      "capture_screen",
      "locate_element",
    ] as const) {
      expect(ComputerActionSchema.parse(action)).toBe(action);
    }
    expect(() => ComputerActionSchema.parse("reboot")).toThrow();
  });

  it("validates targets strictly", () => {
    expect(ComputerTargetSchema.parse({ x: 100, y: 200 })).toBeDefined();
    expect(() => ComputerTargetSchema.parse({ x: -1, y: 5 })).toThrow();
    expect(() =>
      ComputerTargetSchema.parse({ appHint: "x".repeat(300) }),
    ).toThrow();
  });

  it("requires text for type_text and keys for hotkey", () => {
    const base = {
      actionId: "act-1",
      taskId: TaskIdSchema.parse("task-1"),
      target: {},
    } as const;
    expect(() =>
      ComputerActionRequestSchema.parse({
        ...base,
        action: "type_text",
      }),
    ).toThrow();
    expect(() =>
      ComputerActionRequestSchema.parse({
        ...base,
        action: "type_text",
        text: "hello",
      }),
    ).toBeDefined();
    expect(() =>
      ComputerActionRequestSchema.parse({
        ...base,
        action: "hotkey",
      }),
    ).toThrow();
    expect(() =>
      ComputerActionRequestSchema.parse({
        ...base,
        action: "hotkey",
        keys: "ctrl+c",
      }),
    ).toBeDefined();
  });

  it("gates high-risk kinds behind the flag", () => {
    const request = {
      actionId: "act-2",
      taskId: TaskIdSchema.parse("task-1"),
      action: "click",
      target: { x: 10, y: 10 },
      highRisk: true,
      highRiskKind: "destructive",
      dryRun: true,
    } as const;
    expect(ComputerActionRequestSchema.parse(request)).toEqual(request);
    expect(() =>
      ComputerActionRequestSchema.parse({ ...request, highRisk: false }),
    ).toThrow();
  });

  it("parses computer receipts with hashes and bounding boxes", () => {
    const receipt = {
      id: "receipt-1",
      actionId: "act-1",
      taskId: TaskIdSchema.parse("task-1"),
      action: "move_mouse",
      status: "success",
      dryRun: true,
      screenHash: "sha256:abc",
      boundingBox: { x: 1, y: 2, width: 3, height: 4 },
      confidence: 0.9,
      policyDecision: "Allow",
      policyReason: "allowed",
      executedAt: NOW,
    } as const;
    expect(ComputerReceiptSchema.parse(receipt)).toEqual(receipt);
    expect(() =>
      ComputerReceiptSchema.parse({ ...receipt, status: "maybe" }),
    ).toThrow();
  });
});
