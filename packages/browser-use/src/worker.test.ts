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

  it("lists and clears gateway profiles without a browser", async () => {
    const root = mkdtempSync(join(tmpdir(), "talos-worker-profiles-"));
    const server = new WorkerServer(
      new LiveBrowserAdapter({
        launch: async () => {
          throw new Error("no browser in unit tests");
        },
      }),
    );
    const listed = await server.handle({
      id: "p1",
      method: "profile.list",
      params: { userDataRoot: root },
    });
    expect(listed.response.ok).toBe(true);
    expect(
      (listed.response.result as { profiles: unknown[] }).profiles,
    ).toEqual([]);
    const cleared = await server.handle({
      id: "p2",
      method: "profile.clear",
      params: { profile: "default", userDataRoot: root },
    });
    expect(cleared.response.ok).toBe(true);
    expect((cleared.response.result as { cleared: boolean }).cleared).toBe(
      false,
    );
  });

  it("pauses for a human and resumes on signal", async () => {
    const server = new WorkerServer(
      new LiveBrowserAdapter({
        launch: async () => {
          throw new Error("no browser in unit tests");
        },
      }),
    );
    // Pause: the receipt is immediate so the stdio client sees the modal
    // event without waiting for the verdict.
    const paused = await server.handle({
      id: "h1",
      method: "browser.wait_for_human",
      params: {
        taskId: "task-hitl-1",
        kind: "captcha_detected",
        reason: "captcha-challenge",
        url: "https://example.com/protected",
      },
    });
    expect(paused.response.ok).toBe(true);
    const receipt = paused.response.result as {
      status: string;
      interventionId: string;
    };
    expect(receipt.status).toBe("paused");
    expect(receipt.interventionId.startsWith("hitl-")).toBe(true);
    expect(paused.events.length).toBe(1);
    // Status poll observes the pending gate (execution stays paused).
    const pending = await server.handle({
      id: "h2",
      method: "browser.hitl_status",
      params: { interventionId: receipt.interventionId },
    });
    expect(pending.response.result).toMatchObject({ pending: true });
    // Resume: the human solved the challenge in the foreground window.
    const resumed = await server.handle({
      id: "h3",
      method: "browser.resume_human",
      params: { interventionId: receipt.interventionId, resumed: true },
    });
    expect(resumed.response.result).toMatchObject({
      status: "success",
      resumed: true,
    });
    // The verdict lands on the next status poll: the agent continues.
    const done = await server.handle({
      id: "h4",
      method: "browser.hitl_status",
      params: { interventionId: receipt.interventionId },
    });
    expect(done.response.result).toMatchObject({
      pending: false,
      resumed: true,
    });
  });

  it("rejects resume for unknown interventions", async () => {
    const server = new WorkerServer(
      new LiveBrowserAdapter({
        launch: async () => {
          throw new Error("no browser in unit tests");
        },
      }),
    );
    const { response } = await server.handle({
      id: "h9",
      method: "browser.resume_human",
      params: { interventionId: "hitl-nope", resumed: true },
    });
    expect(response.ok).toBe(true);
    expect((response.result as { error: { code: string } }).error.code).toBe(
      "unknown_intervention",
    );
  });
});
