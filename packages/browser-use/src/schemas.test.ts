import { describe, expect, it } from "vitest";
import {
  BrowserActionRequestSchema,
  BrowserReceiptSchema,
  BrowserTargetSchema,
} from "@talos/contracts";

describe("browser schemas", () => {
  it("validates targets", () => {
    expect(
      BrowserTargetSchema.parse({ url: "https://example.com" }),
    ).toBeDefined();
    expect(() =>
      BrowserTargetSchema.parse({ url: "javascript:alert(1)" }),
    ).toThrow();
  });

  it("validates action requests and receipts", () => {
    const request = BrowserActionRequestSchema.parse({
      actionId: "act-1",
      taskId: "task-1",
      action: "snapshot_dom",
      target: { url: "https://example.com" },
      dryRun: true,
    });
    expect(request.action).toBe("snapshot_dom");
    const approved = BrowserActionRequestSchema.parse({
      actionId: "act-2",
      taskId: "task-1",
      action: "click",
      target: { url: "https://example.com", selector: "button" },
      highRisk: true,
      highRiskKind: "form_submit",
      approved: true,
      thumbnail: true,
      allowedOrigins: ["https://example.com"],
    });
    expect(approved.approved).toBe(true);
    expect(() =>
      BrowserReceiptSchema.parse({ ...request, status: "maybe" }),
    ).toThrow();
  });
});
