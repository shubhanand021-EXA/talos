import { describe, expect, it, vi } from "vitest";
import { runAction, WorkerServer } from "./worker.js";
import { ensureLiveContext, liveEnabled, LiveBrowserAdapter } from "./live.js";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

describe("browser worker", () => {
  it("runs deterministic actions without network", async () => {
    const receipt = await runAction({
      action: "snapshot_dom",
      url: "https://example.com",
    });
    expect(receipt["status"]).toBe("success");
    expect(receipt["simulated"]).toBe(true);
    expect(String(receipt["domHash"] ?? "")).toMatch(/^sha256:/);
  });

  it("rejects invalid input with structured errors", async () => {
    const receipt = await runAction({ action: "teleport" });
    expect(receipt["status"]).toBe("failed");
    expect((receipt["error"] as { code: string } | undefined)?.code).toBe(
      "invalid_params",
    );
  });

  it("stays disabled without opt-in and playwright", async () => {
    const was = process.env.TALOS_BROWSER_LIVE;
    vi.stubEnv("TALOS_BROWSER_LIVE", "");
    expect(liveEnabled()).toBe(false);
    const result = await ensureLiveContext("/tmp/talos-nope");
    expect(result.available).toBe(false);
    if (was === undefined) {
      vi.unstubAllEnvs();
    } else {
      vi.stubEnv("TALOS_BROWSER_LIVE", was);
    }
  });

  it("serves ping over JSON-RPC", async () => {
    const server = new WorkerServer(
      new LiveBrowserAdapter({
        launch: async () => {
          throw new Error("unused");
        },
      }),
    );
    const { response } = await server.handle({ id: "1", method: "ping" });
    expect(response).toMatchObject({ id: "1", ok: true });
  });

  it("rejects malformed frames with structured errors", async () => {
    const server = new WorkerServer(
      new LiveBrowserAdapter({
        launch: async () => {
          throw new Error("unused");
        },
      }),
    );
    const { response } = await server.handle({ nope: true });
    expect(response.ok).toBe(false);
    expect(response.error?.code).toBe("invalid_params");
  });

  it("routes unknown methods to invalid_params", async () => {
    const server = new WorkerServer(
      new LiveBrowserAdapter({
        launch: async () => {
          throw new Error("unused");
        },
      }),
    );
    const { response } = await server.handle({
      id: "2",
      method: "teleport",
      params: { taskId: "t" },
    });
    expect(response.ok).toBe(false);
  });

  it("manages sessions without a browser when idle", async () => {
    const root = mkdtempSync(join(tmpdir(), "talos-worker-serve-"));
    void root;
    const server = new WorkerServer(
      new LiveBrowserAdapter({
        launch: async () => {
          throw new Error("no browser in unit tests");
        },
      }),
    );
    expect(server.sessionCount()).toBe(0);
    const { response } = await server.handle({
      id: "3",
      method: "session.status",
      params: {},
    });
    expect(response.ok).toBe(true);
  });
});
