import { describe, expect, it } from "vitest";
import {
  allowedOrigins,
  assertNavigableUrl,
  BrowserGuardError,
  isCredentialTarget,
  isSensitiveUrl,
  sensitiveHosts,
} from "./guard.js";

describe("browser guards", () => {
  it("blocks privileged schemes", () => {
    for (const url of [
      "file:///etc/passwd",
      "chrome://settings",
      "about:blank",
      "javascript:alert(1)",
      "data:text/html,<h1>x</h1>",
      "chrome-extension://abc/page.html",
    ]) {
      expect(() => assertNavigableUrl(url)).toThrow(BrowserGuardError);
    }
  });

  it("allows http(s) and returns the origin", () => {
    expect(assertNavigableUrl("https://example.com/a?b=c")).toBe(
      "https://example.com",
    );
  });

  it("enforces the origin allowlist", () => {
    expect(() =>
      assertNavigableUrl("https://evil.test/", ["https://example.com"]),
    ).toThrowError(/origin_not_allowed|outside the task allowlist/);
    expect(
      assertNavigableUrl("https://example.com/", ["https://example.com"]),
    ).toBe("https://example.com");
  });

  it("detects sensitive hosts including subdomains", () => {
    expect(isSensitiveUrl("https://bank.example/", ["bank.example"])).toBe(
      true,
    );
    expect(isSensitiveUrl("https://a.bank.example/", ["bank.example"])).toBe(
      true,
    );
    expect(isSensitiveUrl("https://example.com/", ["bank.example"])).toBe(
      false,
    );
    expect(isSensitiveUrl("not a url", [])).toBe(true);
  });

  it("reads operator lists from the environment", () => {
    expect(sensitiveHosts()).toEqual([]);
    expect(allowedOrigins()).toEqual([]);
  });

  it("flags credential targets without touching values", () => {
    expect(isCredentialTarget('input[type="password"]')).toBe(true);
    expect(isCredentialTarget("#login-password")).toBe(true);
    expect(isCredentialTarget("button#submit")).toBe(false);
    expect(isCredentialTarget(undefined)).toBe(false);
  });
});
