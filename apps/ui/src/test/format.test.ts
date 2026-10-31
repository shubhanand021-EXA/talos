import { describe, expect, it } from "vitest";
import { formatTime, formatUptime, shortId } from "../lib/format.js";

describe("format helpers", () => {
  it("formats ISO times and rejects garbage", () => {
    expect(formatTime("2026-10-31T12:34:56.000Z")).toBe("12:34:56");
    expect(formatTime("garbage")).toBe("—");
  });

  it("formats uptime compactly", () => {
    expect(formatUptime(3661)).toBe("1h 1m");
    expect(formatUptime(90)).toBe("1m 30s");
    expect(formatUptime(7)).toBe("7s");
    expect(formatUptime(-1)).toBe("—");
  });

  it("shortens long ids", () => {
    expect(shortId("agent-orchestrator-1")).toBe("agent-or…");
    expect(shortId("abc")).toBe("abc");
  });
});
