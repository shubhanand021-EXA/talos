import { gzipSync } from "node:zlib";
import { join } from "node:path";
import { readdirSync, statSync } from "node:fs";
import type { AuditAgent, Finding } from "../types.ts";
import { fileExists, readJson, readText } from "../util.ts";

function dirBytes(dir: string): number {
  let total = 0;
  let entries: string[] = [];
  try {
    entries = readdirSync(dir);
  } catch {
    return 0;
  }
  for (const entry of entries) {
    const full = join(dir, entry);
    try {
      const stat = statSync(full);
      if (stat.isDirectory()) {
        total += dirBytes(full);
      } else {
        total += stat.size;
      }
    } catch {
      continue;
    }
  }
  return total;
}

/** Performance: dist budgets, gzip estimates, lazy-chunk budget, heavy dependency watchlist. */
export const performanceAgent: AuditAgent = {
  name: "PerformanceAuditAgent",
  run: async (ctx) => {
    const findings: Finding[] = [];
    const { root, config, record } = ctx;
    const dist = join(root, "apps/ui/dist");

    if (!fileExists(join(dist, "index.html"))) {
      findings.push({
        severity: "warning",
        check: "dist-present",
        message:
          "UI dist missing — run pnpm build first; size checks skipped (not a product failure)",
        evidence: "apps/ui/dist/",
      });
      return findings;
    }
    findings.push({
      severity: "info",
      check: "dist-present",
      message: "UI production bundle present",
      evidence: "apps/ui/dist/index.html",
    });

    const total = dirBytes(dist);
    record("ui.distBytes", total);
    if (total > config.thresholds.maxDistBytes) {
      findings.push({
        severity: "warning",
        check: "dist-budget",
        message: `dist is ${total} bytes, over budget ${config.thresholds.maxDistBytes}`,
        evidence: `apps/ui/dist (${total} B)`,
      });
    } else {
      findings.push({
        severity: "info",
        check: "dist-budget",
        message: `dist is ${total} bytes (budget ${config.thresholds.maxDistBytes})`,
        evidence: `apps/ui/dist (${total} B)`,
      });
    }

    const assetsDir = join(dist, "assets");
    let entries: string[] = [];
    try {
      entries = readdirSync(assetsDir);
    } catch {
      entries = [];
    }
    const mainJs = entries
      .filter((e) => e.endsWith(".js") && !e.startsWith("void-neon"))
      .sort();
    const voidNeonJs = entries
      .filter((e) => e.endsWith(".js") && e.startsWith("void-neon"))
      .sort();
    let mainGzipMax = 0;
    for (const file of mainJs) {
      try {
        const raw = readText(join(assetsDir, file));
        const gzipped = gzipSync(raw).length;
        mainGzipMax = Math.max(mainGzipMax, gzipped);
        if (gzipped > config.thresholds.maxMainJsGzipBytes) {
          findings.push({
            severity: "warning",
            check: "main-bundle-gzip",
            message: `${file} gzip ≈ ${gzipped} B, over budget ${config.thresholds.maxMainJsGzipBytes}`,
            evidence: `apps/ui/dist/assets/${file}`,
          });
        } else {
          findings.push({
            severity: "info",
            check: "main-bundle-gzip",
            message: `${file} gzip ≈ ${gzipped} B (budget ${config.thresholds.maxMainJsGzipBytes})`,
            evidence: `apps/ui/dist/assets/${file}`,
          });
        }
      } catch {
        findings.push({
          severity: "warning",
          check: "main-bundle-gzip",
          message: `Could not read ${file} for gzip estimate`,
          evidence: `apps/ui/dist/assets/${file}`,
        });
      }
    }
    record("ui.mainJsGzipBytes", mainGzipMax);

    // Lazy 3D chunk budget: the Void Neon viewport must stay lazy-loaded so
    // the first paint never pays for three.js.
    let voidNeonGzipMax = 0;
    for (const file of voidNeonJs) {
      try {
        const raw = readText(join(assetsDir, file));
        const gzipped = gzipSync(raw).length;
        voidNeonGzipMax = Math.max(voidNeonGzipMax, gzipped);
        findings.push({
          severity: "info",
          check: "void-neon-chunk-gzip",
          message: `Lazy chunk ${file} gzip ≈ ${gzipped} B`,
          evidence: `apps/ui/dist/assets/${file}`,
        });
      } catch {
        findings.push({
          severity: "warning",
          check: "void-neon-chunk-gzip",
          message: `Could not read ${file} for gzip estimate`,
          evidence: `apps/ui/dist/assets/${file}`,
        });
      }
    }
    if (voidNeonJs.length === 0) {
      findings.push({
        severity: "warning",
        check: "void-neon-chunk-gzip",
        message:
          "No void-neon* lazy chunk found — the 3D viewport may have merged into the main bundle",
        evidence: "apps/ui/dist/assets/",
      });
    } else {
      record("ui.voidNeonChunkGzipBytes", voidNeonGzipMax);
    }

    const uiPkgPath = join(root, "apps/ui/package.json");
    if (fileExists(uiPkgPath)) {
      const ui = readJson<{ dependencies?: Record<string, string> }>(uiPkgPath);
      for (const [dep, note] of Object.entries(config.heavyDependencies)) {
        if (ui.dependencies?.[dep] !== undefined) {
          findings.push({
            severity: "info",
            check: "heavy-deps",
            message: `Heavy dependency "${dep}" present — ${note}`,
            evidence: "apps/ui/package.json",
          });
        }
      }
    }
    return findings;
  },
};
