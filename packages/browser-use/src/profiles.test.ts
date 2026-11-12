import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import {
  clearProfile,
  ensureProfile,
  gatewayProfileRoot,
  listProfiles,
  sanitizeProfileName,
} from "./profiles.js";

function scratch(): string {
  return mkdtempSync(join(tmpdir(), "talos-profiles-test-"));
}

describe("browser profiles", () => {
  it("resolves the gateway root from the environment", async () => {
    const root = scratch();
    const before = process.env.TALOS_BROWSER_PROFILE_ROOT;
    process.env.TALOS_BROWSER_PROFILE_ROOT = root;
    try {
      expect(gatewayProfileRoot()).toBe(root);
      const ensured = await ensureProfile("default");
      expect(ensured.persistent).toBe(true);
      expect(ensured.userDataDir).toBe(join(root, "default"));
    } finally {
      if (before === undefined) {
        delete process.env.TALOS_BROWSER_PROFILE_ROOT;
      } else {
        process.env.TALOS_BROWSER_PROFILE_ROOT = before;
      }
    }
  });

  it("persists state across ensure calls", async () => {
    const root = scratch();
    const first = await ensureProfile("default", root);
    const { writeFile } = await import("node:fs/promises");
    await writeFile(join(first.userDataDir, "Cookies"), "jar-bytes", "utf8");
    const second = await ensureProfile("default", root);
    expect(second.userDataDir).toBe(first.userDataDir);
    expect(second.createdAt).toBe(first.createdAt);
    const listed = await listProfiles(root);
    expect(listed.map((profile) => profile.name)).toEqual(["default"]);
  });

  it("clears profile state and reports absence", async () => {
    const root = scratch();
    await ensureProfile("default", root);
    expect(await clearProfile("default", root)).toBe(true);
    expect(await clearProfile("default", root)).toBe(false);
    expect(await listProfiles(root)).toEqual([]);
    expect(await listProfiles(join(root, "missing"))).toEqual([]);
  });

  it("rejects traversal and bad names", async () => {
    const root = scratch();
    await expect(ensureProfile("../evil", root)).rejects.toThrow();
    await expect(ensureProfile("a/b", root)).rejects.toThrow();
    await expect(ensureProfile("", root)).rejects.toThrow();
    expect(() => sanitizeProfileName("ok-name_1")).not.toThrow();
  });
});
