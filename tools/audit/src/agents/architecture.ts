import { join } from "node:path";
import type { AuditAgent, Finding } from "../types.ts";
import { fileExists, grepFiles, readJson } from "../util.ts";

const UI_SRC = "apps/ui/src";

/** Architecture: layout, contracts, UI/backend separation, policy centrality. */
export const architectureAgent: AuditAgent = {
  name: "ArchitectureAuditAgent",
  run: async (ctx) => {
    const findings: Finding[] = [];
    const { root } = ctx;

    const requiredDirs = [
      "apps/desktop",
      "apps/ui",
      "crates",
      "crates/talos_common",
      "crates/policy_broker",
      "crates/evidence_store",
      "crates/agent_runtime",
      "packages/contracts",
      "tools",
      "tools/audit",
      "docs",
    ];
    for (const dir of requiredDirs) {
      if (!fileExists(join(root, dir))) {
        findings.push({
          severity: "critical",
          check: "monorepo-layout",
          message: `Required directory missing: ${dir}`,
          evidence: dir,
        });
      }
    }
    if (!findings.some((f) => f.check === "monorepo-layout")) {
      findings.push({
        severity: "info",
        check: "monorepo-layout",
        message: "All required monorepo directories present",
        evidence: requiredDirs.join(", "),
      });
    }

    const contractsPkg = join(root, "packages/contracts/package.json");
    if (!fileExists(contractsPkg)) {
      findings.push({
        severity: "critical",
        check: "contracts-package",
        message: "Shared contracts package missing",
        evidence: "packages/contracts/package.json",
      });
    } else {
      const pkg = readJson<{ name?: string }>(contractsPkg);
      if (pkg.name !== "@talos/contracts") {
        findings.push({
          severity: "critical",
          check: "contracts-package",
          message: `Contracts package misnamed: ${pkg.name ?? "(none)"}`,
          evidence: "packages/contracts/package.json",
        });
      } else {
        findings.push({
          severity: "info",
          check: "contracts-package",
          message: "@talos/contracts present as single source of truth",
          evidence: "packages/contracts/package.json",
        });
      }
    }

    const leaks = grepFiles(
      root,
      /from\s+["'](policy_broker|evidence_store|agent_runtime|sqlx)["']|require\(["'](policy_broker|evidence_store)["']\)/,
      [UI_SRC],
    );
    for (const hit of leaks.slice(0, 10)) {
      findings.push({
        severity: "critical",
        check: "no-backend-in-ui",
        message: "Rust backend import leaked into UI sources",
        evidence: `${hit.file}:${hit.line}`,
      });
    }
    if (leaks.length === 0) {
      findings.push({
        severity: "info",
        check: "no-backend-in-ui",
        message: "UI imports no Rust backend crates",
        evidence: `${UI_SRC}/**/*.{ts,tsx}`,
      });
    }

    const rawInvokes = grepFiles(
      root,
      /(?<!invokeCommand\()(?<!\.)\binvoke\s*\(/,
      [UI_SRC],
    ).filter(
      (hit) =>
        !hit.file.includes("src/bridge/") && !hit.file.endsWith(".test.ts"),
    );
    for (const hit of rawInvokes.slice(0, 10)) {
      findings.push({
        severity: "warning",
        check: "bridge-only-invoke",
        message:
          "Raw Tauri invoke() outside src/bridge — route through the typed bridge",
        evidence: `${hit.file}:${hit.line}`,
      });
    }
    if (rawInvokes.length === 0) {
      findings.push({
        severity: "info",
        check: "bridge-only-invoke",
        message: "All Tauri calls flow through src/bridge",
        evidence: `${UI_SRC}/bridge/`,
      });
    }

    const engineRefs = grepFiles(root, /DefaultPolicyEngine|PolicyEngine/, [
      "apps/desktop/src-tauri/src",
    ]);
    if (engineRefs.length === 0) {
      findings.push({
        severity: "critical",
        check: "policy-centrality",
        message:
          "No PolicyEngine usage in the Tauri shell — policy broker bypassed",
        evidence: "apps/desktop/src-tauri/src",
      });
    } else {
      findings.push({
        severity: "info",
        check: "policy-centrality",
        message: `Policy broker referenced ${engineRefs.length}× in the Tauri shell — central authority intact`,
        evidence: engineRefs[0]?.file ?? "apps/desktop/src-tauri/src",
      });
    }
    return findings;
  },
};
