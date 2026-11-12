import { describe, expect, it } from "vitest";
import {
  BrowserActionRequestSchema,
  BrowserActionSchema,
  BrowserProfileSchema,
  BrowserReceiptSchema,
  BrowserTargetSchema,
  HighRiskKindSchema,
  HitlBrowserEventSchema,
  HitlKindSchema,
  SelectorReportSchema,
} from "./browser.js";
import { AgentIdSchema, TaskIdSchema } from "./identity.js";

const NOW = "2026-10-31T00:00:00.000Z";

describe("browser domain", () => {
  it("accepts all eight browser actions", () => {
    for (const action of [
      "navigate",
      "click",
      "type",
      "scroll",
      "select",
      "extract_text",
      "screenshot",
      "snapshot_dom",
    ] as const) {
      expect(BrowserActionSchema.parse(action)).toBe(action);
    }
    expect(() => BrowserActionSchema.parse("download_ram")).toThrow();
  });

  it("validates targets strictly", () => {
    expect(
      BrowserTargetSchema.parse({ url: "https://example.com" }),
    ).toBeDefined();
    expect(() =>
      BrowserTargetSchema.parse({ url: "javascript:alert(1)" }),
    ).toThrow();
    expect(() =>
      BrowserTargetSchema.parse({ url: "https://user:pass@example.com" }),
    ).toThrow();
    expect(() => BrowserTargetSchema.parse({ url: "" })).toThrow();
  });

  it("parses action requests and gates high-risk flags", () => {
    const request = {
      actionId: "act-1",
      taskId: TaskIdSchema.parse("task-1"),
      action: "click",
      target: { url: "https://example.com", selector: "button#buy" },
      highRisk: true,
      highRiskKind: "purchase",
      dryRun: true,
    } as const;
    expect(BrowserActionRequestSchema.parse(request)).toEqual(request);
    // highRiskKind without highRisk is rejected.
    expect(() =>
      BrowserActionRequestSchema.parse({ ...request, highRisk: false }),
    ).toThrow();
    // javascript: URLs are rejected.
    expect(() =>
      BrowserActionRequestSchema.parse({
        ...request,
        target: { url: "javascript:void(0)" },
      }),
    ).toThrow();
    for (const kind of [
      "form_submit",
      "purchase",
      "settings_change",
      "file_upload",
      "file_download",
      "destructive",
      "other",
    ] as const) {
      expect(HighRiskKindSchema.parse(kind)).toBe(kind);
    }
  });

  it("parses selector reports and receipts", () => {
    const report = {
      selector: "button#buy",
      found: true,
      visible: true,
      interactable: true,
      matchCount: 1,
      domHash: "sha256:abc",
    } as const;
    expect(SelectorReportSchema.parse(report)).toEqual(report);
    const receipt = {
      id: "receipt-1",
      actionId: "act-1",
      taskId: TaskIdSchema.parse("task-1"),
      action: "screenshot",
      status: "success",
      dryRun: true,
      domHash: "sha256:dom",
      screenshotHash: "sha256:shot",
      targetUrl: "https://example.com",
      policyDecision: "Allow",
      policyReason: "allowed",
      executionFidelity: "mock",
      executedAt: NOW,
    } as const;
    expect(BrowserReceiptSchema.parse(receipt)).toEqual(receipt);
    expect(() =>
      BrowserReceiptSchema.parse({ ...receipt, executionFidelity: "bogus" }),
    ).toThrow();
    expect(() =>
      BrowserReceiptSchema.parse({
        ...receipt,
        agentId: AgentIdSchema.parse("agent-1"),
        extra: true,
      }),
    ).toThrow();
  });

  it("parses persistent profiles and HITL intervention events", () => {
    const profile = {
      name: "default",
      userDataDir: "/home/talos/.talos/browser_profiles/default",
      persistent: true,
      createdAt: NOW,
    } as const;
    expect(BrowserProfileSchema.parse(profile)).toEqual(profile);
    expect(() =>
      BrowserProfileSchema.parse({ ...profile, persistent: false }),
    ).toThrow();
    for (const kind of ["captcha_detected", "auth_required"] as const) {
      expect(HitlKindSchema.parse(kind)).toBe(kind);
    }
    const event = {
      interventionId: "hitl-1",
      taskId: TaskIdSchema.parse("task-1"),
      kind: "captcha_detected",
      url: "https://example.com/protected",
      reason: "cloudflare-challenge",
      at: NOW,
    } as const;
    expect(HitlBrowserEventSchema.parse(event)).toEqual(event);
    // No page content or secrets ride the event: strict shape rejects extras.
    expect(() =>
      HitlBrowserEventSchema.parse({ ...event, screenshot: "bytes" }),
    ).toThrow();
    expect(() =>
      HitlBrowserEventSchema.parse({ ...event, reason: "" }),
    ).toThrow();
  });
});
