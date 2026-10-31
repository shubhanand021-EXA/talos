import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

const requiredFiles = [
  "package.json",
  "pnpm-workspace.yaml",
  "tsconfig.base.json",
  "justfile",
  "README.md",
  "LICENSE",
  ".gitignore",
  ".editorconfig",
  ".nvmrc",
  "docs/ARCHITECTURE.md",
  "docs/PROGRESS.md",
  "docs/EVIDENCE.md",
  "docs/ROADMAP.md",
];

const requiredDirs = [
  "apps/desktop",
  "apps/ui",
  "crates",
  "packages/contracts",
  "tools",
  "docs",
  ".github/workflows",
];

describe("talos green baseline", () => {
  it("has required files", () => {
    for (const f of requiredFiles) {
      assert.ok(existsSync(join(root, f)), `missing file: ${f}`);
    }
  });

  it("has required directories", () => {
    for (const d of requiredDirs) {
      assert.ok(existsSync(join(root, d)), `missing dir: ${d}`);
    }
  });

  it("has valid workspace config", () => {
    const pkg = JSON.parse(readFileSync(join(root, "package.json"), "utf8"));
    assert.equal(pkg.name, "talos");
    assert.equal(pkg.private, true);
    assert.match(pkg.packageManager, /^pnpm@/);
    for (const s of [
      "setup",
      "lint",
      "format",
      "typecheck",
      "test",
      "build",
      "audit",
    ]) {
      assert.ok(pkg.scripts[s], `missing script: ${s}`);
    }
  });

  it("has strict TypeScript base", () => {
    const ts = JSON.parse(
      readFileSync(join(root, "tsconfig.base.json"), "utf8"),
    );
    const opts = ts.compilerOptions;
    assert.equal(opts.strict, true);
    assert.equal(opts.noUncheckedIndexedAccess, true);
    assert.equal(opts.exactOptionalPropertyTypes, true);
    assert.equal(opts.noImplicitOverride, true);
  });
});
