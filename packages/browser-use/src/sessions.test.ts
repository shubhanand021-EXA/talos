import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it, vi } from "vitest";
import type { LiveBrowserContext } from "./live.js";
import { LiveSessionManager } from "./sessions.js";

function fakeContext(): LiveBrowserContext & { closed: boolean } {
  const context = {
    closed: false,
    pages: () => [],
    newPage: async () => {
      throw new Error("unused in session tests");
    },
    close: async () => {
      context.closed = true;
    },
  };
  return context;
}

describe("live session manager", () => {
  it("launches isolated sessions per task", async () => {
    const root = mkdtempSync(join(tmpdir(), "talos-sessions-"));
    const manager = new LiveSessionManager({
      maxSessions: 4,
      launch: async () => fakeContext(),
    });
    const a = await manager.launchSession("task-a", root);
    const b = await manager.launchSession("task-b", root);
    expect(a.session.profileDir).not.toBe(b.session.profileDir);
    expect(a.session.taskId).toBe("task-a");
    expect(manager.count()).toBe(2);
    expect(await manager.closeSession("task-a")).toBe(true);
    expect(await manager.closeSession("task-a")).toBe(false);
    expect(await manager.closeSession("task-b")).toBe(true);
  });

  it("reuses the warm session for the same task", async () => {
    const launched: string[] = [];
    const manager = new LiveSessionManager({
      launch: async (dir: string) => {
        launched.push(dir);
        return fakeContext();
      },
    });
    const first = await manager.launchSession("task-a");
    const second = await manager.launchSession("task-a");
    expect(second).toBe(first);
    expect(launched).toHaveLength(1);
  });

  it("enforces the single-session cap", async () => {
    const manager = new LiveSessionManager({
      maxSessions: 1,
      launch: async () => fakeContext(),
    });
    await manager.launchSession("task-a");
    await expect(manager.launchSession("task-b")).rejects.toThrowError(
      /session limit/,
    );
  });

  it("auto-closes idle sessions on sweep", async () => {
    let now = 1_000_000;
    const closed: string[] = [];
    const manager = new LiveSessionManager({
      idleTimeoutMs: 60_000,
      now: () => now,
      launch: async () => {
        const context = fakeContext();
        const original = context.close.bind(context);
        context.close = async () => {
          closed.push("task-idle");
          await original();
        };
        return context;
      },
    });
    await manager.launchSession("task-idle");
    now += 61_000;
    expect(await manager.sweepIdle()).toEqual(["task-idle"]);
    expect(closed).toEqual(["task-idle"]);
    expect(manager.count()).toBe(0);
  });

  it("closes everything on emergency stop", async () => {
    const manager = new LiveSessionManager({
      maxSessions: 4,
      launch: async () => fakeContext(),
    });
    await manager.launchSession("t1");
    await manager.launchSession("t2");
    expect(await manager.closeAll()).toEqual(["t1", "t2"]);
    expect(manager.count()).toBe(0);
  });

  it("rejects invalid configuration", () => {
    expect(
      () =>
        new LiveSessionManager({
          maxSessions: 0,
          launch: async () => fakeContext(),
        }),
    ).toThrow();
    expect(() => new LiveSessionManager({})).toThrow();
    expect(vi.fn()).toBeDefined();
  });
});
