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
    // Rationale for allowlisted documentation domains (written exclusion):
    // reserved names such as example.com (RFC 2606) appear only in UI
    // placeholder JSON, Rust test/demo fixtures, and executor dry-run
    // samples — never as a runtime fetch target. The whole reserved
    // .example TLD (RFC 2606 section 3, never delegated, never routable)
    // is likewise documentation-only; it shows up in #[cfg(test)] mock-DOM
    // fixtures. Any other non-allowlisted https:// URL still warns.
    const reservedDoc = String.raw`[\w.-]*\.example\b|example\.(?:com|net|org)\b`;
    const remoteRe = new RegExp(
      `https://(?!${allowed}|${reservedDoc})([^\\s'";]+)`,
      "i",
    );
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

    // Browser automation must stay profile-isolated: per-task dirs, never the
    // personal profile. Static marker check (runtime isolation is covered by
    // browser-use package tests).
    if (!fileExists(join(root, "packages/browser-use/src"))) {
      findings.push({
        severity: "info",
        check: "browser-profile-isolation",
        message: "Browser-use sources absent here — isolation unverifiable",
        evidence: "packages/browser-use/src",
      });
    } else {
      const isolation = grepFiles(
        root,
        /isolated profile|fresh profile|incognito|userDataDir|partition/i,
        ["packages/browser-use/src"],
      );
      if (isolation.length === 0) {
        findings.push({
          severity: "warning",
          check: "browser-profile-isolation",
          message:
            "No profile-isolation markers in browser-use — confirm per-task profiles are documented",
          evidence: "packages/browser-use/src",
        });
      } else {
        findings.push({
          severity: "info",
          check: "browser-profile-isolation",
          message: `Browser sessions stay profile-isolated (${isolation.length} marker(s))`,
          evidence: isolation
            .slice(0, 2)
            .map((h) => `${h.file}:${h.line}`)
            .join(", "),
        });
      }
    }
    return findings;
  },
};
