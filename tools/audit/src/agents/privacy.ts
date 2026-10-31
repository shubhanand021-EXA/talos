import { join } from "node:path";
import type { AuditAgent, Finding } from "../types.ts";
import { fileExists, grepFiles, readJson } from "../util.ts";

function escapeRegExp(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/** Privacy: no remote telemetry, no analytics SDKs, local-first Batch 1. */
export const privacyAgent: AuditAgent = {
  name: "PrivacyAuditAgent",
  run: async (ctx) => {
    const findings: Finding[] = [];
    const { root, config } = ctx;

    for (const pattern of config.telemetryPatterns) {
      const hits = grepFiles(root, new RegExp(pattern, "i"), [
        "apps/ui/src",
        "apps/desktop/src-tauri/src",
        "crates",
      ]);
      for (const hit of hits.slice(0, 10)) {
        findings.push({
          severity: "critical",
          check: "no-telemetry",
          message: `Telemetry pattern /${pattern}/ in source`,
          evidence: `${hit.file}:${hit.line}: ${hit.text}`,
        });
      }
    }
    if (!findings.some((f) => f.check === "no-telemetry")) {
      findings.push({
        severity: "info",
        check: "no-telemetry",
        message: "No telemetry/analytics patterns in code",
        evidence: `${config.telemetryPatterns.length} patterns scanned`,
      });
    }

    const allowed = config.allowedRemoteDomains.map(escapeRegExp).join("|");
    const remoteRe = new RegExp(`https://(?!${allowed})([^\\s'";]+)`, "i");
    const remoteHits = grepFiles(root, remoteRe, [
      "apps/ui/src",
      "apps/desktop/src-tauri/src",
      "crates",
    ]);
    for (const hit of remoteHits.slice(0, 10)) {
      findings.push({
        severity: "warning",
        check: "no-remote-endpoints",
        message:
          "Non-allowlisted https:// URL in code — confirm it is not a runtime call",
        evidence: `${hit.file}:${hit.line}: ${hit.text}`,
      });
    }
    if (remoteHits.length === 0) {
      findings.push({
        severity: "info",
        check: "no-remote-endpoints",
        message: "No remote https:// endpoints in code",
        evidence: `allowlist: ${config.allowedRemoteDomains.join(", ")}`,
      });
    }

    const manifests = [
      "package.json",
      "apps/ui/package.json",
      "apps/desktop/package.json",
    ];
    let flagged = false;
    for (const manifest of manifests) {
      const path = join(root, manifest);
      if (!fileExists(path)) {
        continue;
      }
      const pkg = readJson<{
        dependencies?: Record<string, string>;
        devDependencies?: Record<string, string>;
      }>(path);
      const names = [
        ...Object.keys(pkg.dependencies ?? {}),
        ...Object.keys(pkg.devDependencies ?? {}),
      ];
      for (const dep of config.telemetryDeps) {
        if (names.includes(dep)) {
          flagged = true;
          findings.push({
            severity: "critical",
            check: "no-analytics-sdk",
            message: `Analytics SDK "${dep}" in ${manifest}`,
            evidence: manifest,
          });
        }
      }
    }
    if (!flagged) {
      findings.push({
        severity: "info",
        check: "no-analytics-sdk",
        message: "No analytics SDKs in workspace manifests",
        evidence: manifests.join(", "),
      });
    }
    return findings;
  },
};
