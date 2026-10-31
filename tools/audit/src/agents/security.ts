import { join } from "node:path";
import type { AuditAgent, Finding } from "../types.ts";
import { fileExists, grepFiles, readJson } from "../util.ts";

function secretFindings(root: string, patterns: string[]): Finding[] {
  // Pattern definitions (.talos/), generated reports, and test fixtures
  // (*.test.*) are not scannable secrets; production code paths are what matter.
  const exclude = [".talos", "tests", "fixtures", "docs/reports"];
  const findings: Finding[] = [];
  for (const source of patterns) {
    let re: RegExp;
    try {
      re = new RegExp(source);
    } catch {
      findings.push({
        severity: "warning",
        check: "secret-pattern-valid",
        message: `Forbidden pattern does not compile: ${source}`,
        evidence: ".talos/audit.config.json",
      });
      continue;
    }
    const hits = grepFiles(root, re, undefined, exclude);
    for (const hit of hits) {
      findings.push({
        severity: "critical",
        check: "no-secrets",
        message: `Possible secret matched /${source}/`,
        evidence: `${hit.file}:${hit.line}`,
      });
    }
  }
  return findings;
}

/** Security: unsafe Rust, Tauri capabilities, CSP, secrets, shady deps. */
export const securityAgent: AuditAgent = {
  name: "SecurityAuditAgent",
  run: async (ctx) => {
    const findings: Finding[] = [];
    const { root, config } = ctx;

    const unsafeHits = grepFiles(root, /\bunsafe\b/, ["crates"]);
    if (unsafeHits.length > 0) {
      for (const hit of unsafeHits.slice(0, 10)) {
        findings.push({
          severity: "critical",
          check: "no-unsafe-rust",
          message: "`unsafe` block in Rust sources (workspace forbids it)",
          evidence: `${hit.file}:${hit.line}: ${hit.text}`,
        });
      }
    } else {
      findings.push({
        severity: "info",
        check: "no-unsafe-rust",
        message: "No `unsafe` blocks in crates/",
        evidence: "grep \\bunsafe\\b crates/",
      });
    }

    const capabilities = join(
      root,
      "apps/desktop/src-tauri/capabilities/default.json",
    );
    if (!fileExists(capabilities)) {
      findings.push({
        severity: "critical",
        check: "tauri-capability-present",
        message: "Missing Tauri capability file",
        evidence: "apps/desktop/src-tauri/capabilities/default.json",
      });
    } else {
      const cap = readJson<{ permissions?: unknown; windows?: unknown }>(
        capabilities,
      );
      if (Array.isArray(cap.permissions) && cap.permissions.length > 0) {
        findings.push({
          severity: "critical",
          check: "tauri-least-privilege",
          message: `Capability grants ${cap.permissions.length} permission(s); Batch 1 baseline is zero`,
          evidence: JSON.stringify(cap.permissions).slice(0, 300),
        });
      } else {
        findings.push({
          severity: "info",
          check: "tauri-least-privilege",
          message: "Default capability grants zero extra permissions",
          evidence: "capabilities/default.json: permissions: []",
        });
      }
    }

    const confPath = join(root, "apps/desktop/src-tauri/tauri.conf.json");
    if (fileExists(confPath)) {
      const conf = readJson<{
        app?: { security?: { csp?: string } };
      }>(confPath);
      const csp = conf.app?.security?.csp ?? "";
      const hosts = csp.match(/https?:\/\/[^\s;'"]+/g) ?? [];
      const remote = hosts.filter((h) => !h.startsWith("http://ipc.localhost"));
      if (remote.length > 0) {
        findings.push({
          severity: "critical",
          check: "csp-no-remote",
          message: `Production CSP allows remote hosts: ${remote.join(", ")}`,
          evidence: "tauri.conf.json:app.security.csp",
        });
      } else {
        findings.push({
          severity: "info",
          check: "csp-no-remote",
          message:
            "Production CSP has no remote hosts (only self + ipc.localhost)",
          evidence: "tauri.conf.json:app.security.csp",
        });
      }
    }

    findings.push(...secretFindings(root, config.forbiddenPatterns));

    for (const manifest of [
      "package.json",
      "apps/ui/package.json",
      "apps/desktop/package.json",
    ]) {
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
      for (const dep of config.suspiciousDependencies) {
        if (names.some((n) => n === dep || n.includes(dep))) {
          findings.push({
            severity: "critical",
            check: "no-dangerous-deps",
            message: `Suspicious dependency "${dep}" in ${manifest}`,
            evidence: manifest,
          });
        }
      }
    }
    if (!findings.some((f) => f.check === "no-dangerous-deps")) {
      findings.push({
        severity: "info",
        check: "no-dangerous-deps",
        message: "No denylisted dependencies in workspace manifests",
        evidence: "package.json × 3",
      });
    }
    return findings;
  },
};
