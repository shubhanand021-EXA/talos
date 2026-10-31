import { describe, expect, it } from "vitest";
import { MockBrowserAdapter } from "./mock.js";
import { createSession, sessionExists } from "./session.js";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

function root(): string {
  return mkdtempSync(join(tmpdir(), "talos-browser-test-"));
}

describe("mock browser adapter", () => {
  it("runs a deterministic action lifecycle", async () => {
    const adapter = new MockBrowserAdapter();
    const first = await adapter.liveNavigate(
      "task-1",
      "https://example.com",
      root(),
    );
    const second = await adapter.liveNavigate(
      "task-1",
      "https://example.com",
      root(),
    );
    expect(first["domHash"]).toBe(second["domHash"]);
    expect(first["simulated"]).toBe(true);
  });

  it("dry-run resolves selectors without mutating", () => {
    const adapter = new MockBrowserAdapter();
    const planned = adapter.planClick(undefined, "button#ok");
    expect(planned["planned"]).toBe(true);
    expect(planned["resolved"]).toBeDefined();
    expect(() => adapter.planClick(undefined, ".missing-thing")).toThrow();
  });

  it("produces evidence-shaped receipts", async () => {
    const adapter = new MockBrowserAdapter();
    const shot = adapter.screenshot("https://example.com");
    expect(String(shot["screenshotHash"])).toMatch(/^sha256:/);
    const text = adapter.extract("https://example.com", undefined, 10);
    expect(String(text["text"])).toHaveLength(10);
    expect(text["truncated"]).toBe(true);
  });

  it("keeps sessions isolated per task", async () => {
    const dir = root();
    const a = await createSession("task-a", { root: dir });
    const b = await createSession("task-b", { root: dir });
    expect(a.profileDir).not.toBe(b.profileDir);
    expect(await sessionExists(a)).toBe(true);
    expect(await sessionExists(b)).toBe(true);
  });
});
