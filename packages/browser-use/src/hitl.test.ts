import { describe, expect, it } from "vitest";
import { detectIntervention, hitlEvent } from "./hitl.js";

describe("browser hitl", () => {
  it("detects captcha markers over login forms", () => {
    expect(
      detectIntervention({
        title: "Just a moment",
        html: 'cf-challenge-run <input type="password">',
      }),
    ).toMatchObject({
      kind: "captcha_detected",
      reason: "cloudflare-challenge",
    });
    expect(
      detectIntervention({ html: '<div class="g-recaptcha"></div>' }),
    ).toMatchObject({ kind: "captcha_detected" });
  });

  it("detects login walls", () => {
    expect(
      detectIntervention({
        url: "https://example.com/login",
        html: '<input type="password" name="pw">',
      }),
    ).toMatchObject({ kind: "auth_required", reason: "password-field" });
  });

  it("passes clean pages through", () => {
    expect(
      detectIntervention({
        url: "https://example.com",
        title: "Example",
        html: "<h1>hi</h1>",
      }),
    ).toBeNull();
    expect(detectIntervention({})).toBeNull();
  });

  it("builds bounded, content-free events", () => {
    const event = hitlEvent(
      "task-1",
      "auth_required",
      "login-wall",
      "https://example.com/login",
    );
    expect(event.taskId).toBe("task-1");
    expect(event.kind).toBe("auth_required");
    expect(event.interventionId.startsWith("hitl-")).toBe(true);
    const long = hitlEvent(
      "t",
      "captcha_detected",
      `captcha ${"x".repeat(10_000)}`,
    );
    expect(long.reason.length).toBeLessThanOrEqual(256);
  });
});
