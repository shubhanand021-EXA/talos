import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { describe, it } from "node:test";
import { fileURLToPath } from "node:url";
import { DEFAULT_CONFIG, loadConfig } from "../src/config.ts";

// Repo root derived from this file's location (tools/audit/tests/) so no
// absolute machine path is baked into tracked test code.
const repoRoot = join(
  dirname(fileURLToPath(import.meta.url)),
  "..",
  "..",
  "..",
);

function fixture(): string {
  const dir = mkdtempSync(join(tmpdir(), "talos-audit-config-"));
  mkdirSync(join(dir, ".talos"), { recursive: true });
  return dir;
}

describe("audit config", () => {
  it("loads the repo config and validates shape", () => {
    const config = loadConfig(repoRoot);
    assert.equal(config.version, 1);
    assert.ok(config.thresholds.maxDistBytes > 0);
    assert.ok(config.requiredFiles.includes("package.json"));
  });

  it("throws on missing config", () => {
    const dir = fixture();
    try {
      assert.throws(() => loadConfig(dir), /audit config not found/);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("throws on malformed config", () => {
    const dir = fixture();
    try {
      writeFileSync(
        join(dir, ".talos", "audit.config.json"),
        JSON.stringify({ nope: true }),
      );
      assert.throws(() => loadConfig(dir), /invalid/);
      writeFileSync(
        join(dir, ".talos", "audit.config.json"),
        JSON.stringify({ version: 1, thresholds: {} }),
      );
      const config = loadConfig(dir);
      assert.equal(
        config.thresholds.maxDistBytes,
        DEFAULT_CONFIG.thresholds.maxDistBytes,
      );
      assert.deepEqual(config.allowedLicenses, DEFAULT_CONFIG.allowedLicenses);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });
});
