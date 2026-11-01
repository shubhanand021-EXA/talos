import { mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import {
  countPasswordFields,
  MAX_ARTIFACT_BYTES,
  sha256HexBytes,
  storeArtifact,
} from "./artifacts.js";

describe("capture artifacts", () => {
  it("hashes bytes deterministically", () => {
    const bytes = new Uint8Array([1, 2, 3]);
    expect(sha256HexBytes(bytes)).toMatch(/^sha256:[0-9a-f]{64}$/);
    expect(sha256HexBytes(bytes)).toBe(sha256HexBytes(bytes));
  });

  it("counts password fields", () => {
    expect(countPasswordFields("<html></html>")).toBe(0);
    expect(
      countPasswordFields('<input type="password"><INPUT TYPE=password>'),
    ).toBe(2);
  });

  it("stores content-addressed captures under the profile", async () => {
    const root = mkdtempSync(join(tmpdir(), "talos-artifacts-"));
    const ref = await storeArtifact(root, new Uint8Array([9, 9]), "png", {});
    expect(ref.hash).toMatch(/^sha256:/);
    expect(ref.path).toContain("artifacts");
    expect(readFileSync(ref.path as string)).toHaveLength(2);
  });

  it("skips sensitive pages and oversized captures", async () => {
    const root = mkdtempSync(join(tmpdir(), "talos-artifacts-"));
    const sensitive = await storeArtifact(root, new Uint8Array([1]), "png", {
      sensitive: true,
    });
    expect(sensitive.path).toBeUndefined();
    expect(sensitive.skipped).toBe("sensitive_page");
    const big = await storeArtifact(
      root,
      new Uint8Array(MAX_ARTIFACT_BYTES + 1),
      "png",
      {},
    );
    expect(big.path).toBeUndefined();
    expect(big.skipped).toBe("too_large");
  });
});
