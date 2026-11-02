#!/usr/bin/env node
/**
 * Stage the `talos` CLI binary for Tauri bundling (dependency-free).
 *
 * Release flow:
 *   1. `cargo build --release -p talos_cli` (per target triple)
 *   2. `node scripts/stage-cli-binaries.mjs` — copies each
 *      `target/<triple>/release/talos[.exe]` to
 *      `apps/desktop/src-tauri/binaries/talos-<triple>[.exe]`
 *      (the Tauri `externalBin` sidecar convention)
 *   3. Uncomment `bundle.externalBin` in `tauri.conf.json`
 *      (kept commented in dev so `tauri build` works without staged
 *      binaries) and run `pnpm build:desktop`.
 *
 * Desktop + CI check steps never require staged binaries: the script is a
 * no-op (exit 0) when nothing is built yet, so local `tauri build` keeps
 * working without the CLI.
 *
 * Usage:
 *   node scripts/stage-cli-binaries.mjs [--check]
 *     --check   exit 1 when no binary was staged (release-gate mode)
 */

import { copyFileSync, existsSync, mkdirSync, readdirSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const outDir = join(root, "apps", "desktop", "src-tauri", "binaries");
const check = process.argv.includes("--check");

const TRIPLES = [
  {
    triple: "x86_64-pc-windows-msvc",
    exe: "talos.exe",
    out: "talos-x86_64-pc-windows-msvc.exe",
  },
  {
    triple: "x86_64-apple-darwin",
    exe: "talos",
    out: "talos-x86_64-apple-darwin",
  },
  {
    triple: "aarch64-apple-darwin",
    exe: "talos",
    out: "talos-aarch64-apple-darwin",
  },
  {
    triple: "x86_64-unknown-linux-gnu",
    exe: "talos",
    out: "talos-x86_64-unknown-linux-gnu",
  },
];

let staged = 0;
for (const { triple, exe, out } of TRIPLES) {
  const candidates = [
    join(root, "crates", "target", triple, "release", exe),
    join(root, "target", triple, "release", exe),
  ];
  const found = candidates.find((p) => existsSync(p));
  if (!found) {
    continue;
  }
  mkdirSync(outDir, { recursive: true });
  copyFileSync(found, join(outDir, out));
  console.log(`staged ${out}`);
  staged += 1;
}

// Also surface whatever already sits in binaries/ (idempotent reruns).
if (existsSync(outDir)) {
  for (const file of readdirSync(outDir)) {
    if (file !== ".gitkeep") {
      console.log(`present ${file}`);
    }
  }
}

if (staged === 0) {
  console.log(
    "no CLI binaries built yet — dev bundles skip the sidecar (see docs/INSTALL.md)",
  );
  if (check) {
    process.exitCode = 1;
  }
}
