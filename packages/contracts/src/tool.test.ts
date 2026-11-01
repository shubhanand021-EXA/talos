import { describe, expect, it } from "vitest";
import {
  ToolIdSchema,
  ToolInvocationSchema,
  ToolManifestSchema,
  ToolReceiptSchema,
  ToolStatusSchema,
} from "./tool.js";
import { AgentIdSchema, TaskIdSchema } from "./identity.js";

const NOW = "2026-10-31T00:00:00.000Z";

describe("tool domain", () => {
  it("accepts all twenty-three built-in tool ids", () => {
    for (const id of [
      "fs.read",
      "fs.list",
      "fs.search",
      "fs.write",
      "process.exec",
      "browser.navigate",
      "browser.click",
      "browser.type",
      "browser.scroll",
      "browser.select",
      "browser.extract",
      "browser.screenshot",
      "browser.snapshot",
      "browser.session",
      "computer.move_mouse",
      "computer.click",
      "computer.double_click",
      "computer.drag",
      "computer.type_text",
      "computer.hotkey",
      "computer.scroll",
      "computer.capture_screen",
      "computer.locate_element",
    ] as const) {
      expect(ToolIdSchema.parse(id)).toBe(id);
    }
    expect(() => ToolIdSchema.parse("rm-rf")).toThrow();
  });

  it("accepts statuses", () => {
    for (const s of ["success", "failed", "denied", "disabled"] as const) {
      expect(ToolStatusSchema.parse(s)).toBe(s);
    }
  });

  it("parses manifest, invocation, and receipt", () => {
    const manifest = {
      id: "fs.read",
      name: "fs.read",
      version: "0.1.0",
      description: "Read a file",
      capabilities: ["ReadFile"],
      riskLevel: "Low",
      reversible: true,
      requiresEvidence: false,
      requiresCompensation: false,
      enabledByDefault: true,
      inputSchema: { type: "object" },
      outputSchema: { type: "object" },
    } as const;
    expect(ToolManifestSchema.parse(manifest)).toEqual(manifest);

    const invocation = {
      toolId: "fs.read",
      taskId: TaskIdSchema.parse("task-1"),
      agentId: AgentIdSchema.parse("agent-1"),
      params: { path: "a.txt" },
      dryRun: true,
    } as const;
    expect(ToolInvocationSchema.parse(invocation)).toEqual(invocation);

    const receipt = {
      id: "receipt-1",
      toolId: "fs.read",
      taskId: TaskIdSchema.parse("task-1"),
      dryRun: true,
      status: "success",
      output: { path: "a.txt" },
      policyDecision: "Allow",
      policyReason: "allowed",
      executedAt: NOW,
    } as const;
    expect(ToolReceiptSchema.parse(receipt)).toEqual(receipt);
    expect(() =>
      ToolReceiptSchema.parse({ ...receipt, status: "maybe" }),
    ).toThrow();
  });
});
