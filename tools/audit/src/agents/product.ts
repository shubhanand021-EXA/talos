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

    // README must track implemented Batch 2 features, not just the vision.
    const features = config.featureKeywords;
    if (features.length > 0 && fileExists(readmePath)) {
      const readme = readText(readmePath);
      const missing = features.filter((k) => !readme.includes(k));
      if (missing.length > 0) {
        findings.push({
          severity: "warning",
          check: "readme-matches-features",
          message: `README omits implemented features: ${missing.join(", ")}`,
          evidence: "README.md",
        });
      } else {
        findings.push({
          severity: "info",
          check: "readme-matches-features",
          message: "README documents all tracked Batch 2 features",
          evidence: "README.md",
        });
      }
    }

    // Batch 2 progress must be recorded as dated entries.
    const progressPath = join(root, "docs/PROGRESS.md");
    if (fileExists(progressPath)) {
      const entries = (
        readText(progressPath).match(/^## \d{4}-\d{2}-\d{2} /gm) ?? []
      ).length;
      if (entries < 8) {
        findings.push({
          severity: "warning",
          check: "batch2-objectives",
          message: `PROGRESS.md carries ${entries} dated entries — Batch 2 objectives look under-recorded`,
          evidence: "docs/PROGRESS.md",
        });
      } else {
        findings.push({
          severity: "info",
          check: "batch2-objectives",
          message: `PROGRESS.md carries ${entries} dated entries covering Batch 2`,
          evidence: "docs/PROGRESS.md",
        });
      }
    }

    // Architecture doc must name the systems that actually shipped.
    const archPath = join(root, "docs/ARCHITECTURE.md");
    if (fileExists(archPath)) {
      const arch = readText(archPath);
      const systems = ["tool_rag", "TypeCompact", "Void Neon"];
      const missingSystems = systems.filter((s) => !arch.includes(s));
      if (missingSystems.length > 0) {
        findings.push({
          severity: "warning",
          check: "docs-updated",
          message: `ARCHITECTURE.md omits shipped systems: ${missingSystems.join(", ")}`,
          evidence: "docs/ARCHITECTURE.md",
        });
      } else {
        findings.push({
          severity: "info",
          check: "docs-updated",
          message:
            "ARCHITECTURE.md covers Tool-RAG, TypeCompact, and Void Neon",
          evidence: "docs/ARCHITECTURE.md",
        });
      }
    }

    // The UI baseline snapshot must reference the same live files the
    // dashboard renders, or the panel silently shows stale data.
    const auditPage = join(root, "apps/ui/src/features/audit/AuditPage.tsx");
    if (!fileExists(auditPage)) {
      findings.push({
        severity: "info",
        check: "ui-baseline-snapshot-in-sync",
        message: "Audit page absent here — snapshot sync unverifiable",
        evidence: "apps/ui/src/features/audit/",
      });
    } else {
      const source = readText(auditPage);
      const refsReports = source.includes("audit-latest.json");
      const refsBaselines = source.includes("baselines.json");
      if (!refsReports || !refsBaselines) {
        findings.push({
          severity: "warning",
          check: "ui-baseline-snapshot-in-sync",
          message: `Audit page does not reference live data (reports: ${refsReports}, baselines: ${refsBaselines}) — dashboard may show stale snapshots`,
          evidence: "apps/ui/src/features/audit/AuditPage.tsx",
        });
      } else {
        findings.push({
          severity: "info",
          check: "ui-baseline-snapshot-in-sync",
          message: "Audit page reads the live report + baselines files",
          evidence: "apps/ui/src/features/audit/AuditPage.tsx",
        });
      }
    }
    return findings;
  },
};
