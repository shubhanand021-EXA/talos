import { join, sep } from "node:path";
import type { AuditAgent, Finding } from "../types.ts";
import { fileExists, readText } from "../util.ts";

/** Product alignment: vision, Batch 1 artifacts, living docs, no scope drift. */
export const productAgent: AuditAgent = {
  name: "ProductAlignmentAuditAgent",
  run: async (ctx) => {
    const findings: Finding[] = [];
    const { root, config } = ctx;

    const readmePath = join(root, "README.md");
    if (!fileExists(readmePath)) {
      findings.push({
        severity: "critical",
        check: "vision-readme",
        message: "Root README missing",
        evidence: "README.md",
      });
    } else {
      const readme = readText(readmePath);
      const missing = config.visionKeywords.filter((k) => !readme.includes(k));
      if (missing.length > 0) {
        findings.push({
          severity: "warning",
          check: "vision-readme",
          message: `README omits vision keywords: ${missing.join(", ")}`,
          evidence: "README.md",
        });
      } else {
        findings.push({
          severity: "info",
          check: "vision-readme",
          message: "README carries the Talos vision keywords",
          evidence: "README.md",
        });
      }
    }

    for (const doc of [
      "docs/PROGRESS.md",
      "docs/EVIDENCE.md",
      "docs/ARCHITECTURE.md",
      "docs/ROADMAP.md",
    ]) {
      const path = join(root, doc);
      if (!fileExists(path)) {
        findings.push({
          severity: "critical",
          check: "docs-maintained",
          message: `Required doc missing: ${doc}`,
          evidence: doc,
        });
        continue;
      }
      const content = readText(path);
      if (content.trim().length < 200) {
        findings.push({
          severity: "warning",
          check: "docs-maintained",
          message: `Doc suspiciously thin: ${doc}`,
          evidence: doc,
        });
      }
    }
    const progress = fileExists(join(root, "docs/PROGRESS.md"))
      ? readText(join(root, "docs/PROGRESS.md"))
      : "";
    const dated = (progress.match(/^## \d{4}-\d{2}-\d{2} /gm) ?? []).length;
    if (dated === 0) {
      findings.push({
        severity: "warning",
        check: "docs-maintained",
        message: "PROGRESS.md has no dated entries",
        evidence: "docs/PROGRESS.md",
      });
    } else {
      findings.push({
        severity: "info",
        check: "docs-maintained",
        message: `PROGRESS.md carries ${dated} dated entr${dated === 1 ? "y" : "ies"}`,
        evidence: "docs/PROGRESS.md",
      });
    }

    const artifacts = [
      "apps/desktop",
      "apps/ui",
      "crates/policy_broker",
      "crates/evidence_store",
      "crates/agent_runtime",
      "packages/contracts",
      "tools/audit",
    ];
    for (const artifact of artifacts) {
      if (!fileExists(join(root, artifact))) {
        findings.push({
          severity: "critical",
          check: "batch1-artifacts",
          message: `Batch 1 artifact missing: ${artifact}`,
          evidence: artifact,
        });
      }
    }
    if (!findings.some((f) => f.check === "batch1-artifacts")) {
      findings.push({
        severity: "info",
        check: "batch1-artifacts",
        message: "All Batch 1 artifacts present",
        evidence: artifacts.join(", "),
      });
    }

    for (const dir of config.forbiddenTopLevelDirs) {
      if (fileExists(join(root, ...dir.split("/")))) {
        findings.push({
          severity: "warning",
          check: "no-scope-drift",
          message: `Unexpected top-level scope: ${dir.split(sep).join("/")} — confirm it belongs in Batch 1`,
          evidence: dir,
        });
      }
    }
    if (!findings.some((f) => f.check === "no-scope-drift")) {
      findings.push({
        severity: "info",
        check: "no-scope-drift",
        message: "No out-of-scope top-level directories",
        evidence:
          config.forbiddenTopLevelDirs.join(", ") || "(none configured)",
      });
    }
    return findings;
  },
};
