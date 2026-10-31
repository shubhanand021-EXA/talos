import { describe, expect, it, vi } from "vitest";
import { runAction } from "./worker.js";
import { ensureLiveContext, liveEnabled } from "./live.js";

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
});
