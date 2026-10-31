import { join } from "node:path";
import type { AuditAgent, Finding } from "../types.ts";
import { fileExists, readJson, readText } from "../util.ts";

interface LicenseEntry {
  name: string;
  versions?: string[];
  license?: string;
}

/** Dependency health: audit, licenses, duplicates (network-optional parts degrade to warnings). */
export const dependenciesAgent: AuditAgent = {
  name: "DependencyHealthAuditAgent",
  run: async (ctx) => {
    const findings: Finding[] = [];
    const { root, config, exec } = ctx;

    const audit = exec("pnpm", ["audit", "--audit-level", "high", "--json"], {
      timeoutMs: 120000,
      cwd: root,
    });
    if (audit.exitCode === 0) {
      findings.push({
        severity: "info",
        check: "vulnerability-scan",
        message: "pnpm audit: no high-or-worse vulnerabilities",
        evidence: "pnpm audit --audit-level high",
      });
    } else if (audit.timedOut || audit.exitCode === null) {
      findings.push({
        severity: "warning",
        check: "vulnerability-scan",
        message:
          "pnpm audit unreachable (offline?) — scan deferred, re-run with network",
        evidence:
          audit.stderr.slice(-200) || "audit timed out or failed to launch",
      });
    } else {
      findings.push({
        severity: "critical",
        check: "vulnerability-scan",
        message: "pnpm audit reports high-or-worse vulnerabilities",
        evidence: (
          `STDOUT: ${audit.stdout}\nSTDERR: ${audit.stderr}`.trim() ||
          "(no output)"
        ).slice(0, 800),
      });
    }

    const licenses = exec("pnpm", ["licenses", "list", "--json"], {
      timeoutMs: 60000,
      cwd: root,
    });
    if (licenses.exitCode === 0 && licenses.stdout.trim() !== "") {
      try {
        const byLicense = JSON.parse(licenses.stdout) as Record<
          string,
          LicenseEntry[]
        >;
        const unknown: string[] = [];
        for (const [license, entries] of Object.entries(byLicense)) {
          if (!config.allowedLicenses.includes(license)) {
            unknown.push(`${license} (${entries.length} pkg)`);
          }
        }
        if (unknown.length > 0) {
          findings.push({
            severity: "warning",
            check: "license-allowlist",
            message: `Non-allowlisted licenses in tree: ${unknown.slice(0, 8).join(", ")}${unknown.length > 8 ? ` (+${unknown.length - 8} more)` : ""}`,
            evidence: "pnpm licenses list",
          });
        } else {
          findings.push({
            severity: "info",
            check: "license-allowlist",
            message: `All dependency licenses within allowlist (${config.allowedLicenses.length} SPDX ids)`,
            evidence: "pnpm licenses list",
          });
        }
      } catch {
        findings.push({
          severity: "warning",
          check: "license-allowlist",
          message:
            "Could not parse pnpm licenses output — manual review advised",
          evidence: licenses.stdout.slice(0, 200),
        });
      }
    } else {
      findings.push({
        severity: "info",
        check: "license-allowlist",
        message: "License inventory unavailable — manual review advised",
        evidence: (licenses.stderr || "pnpm licenses failed").slice(0, 200),
      });
    }

    // License compatibility: every workspace manifest declares an OSI-approved
    // license from the allowlist (root + workspaces, not just the tree).
    const manifests = [
      "package.json",
      "apps/ui/package.json",
      "apps/desktop/package.json",
      "packages/contracts/package.json",
      "packages/browser-use/package.json",
      "tools/audit/package.json",
    ];
    const incompatible: string[] = [];
    let declared = 0;
    for (const manifest of manifests) {
      const path = join(root, manifest);
      if (!fileExists(path)) {
        continue;
      }
      const pkg = readJson<{ license?: unknown }>(path);
      if (typeof pkg.license !== "string" || pkg.license === "") {
        incompatible.push(`${manifest}: no license field`);
        continue;
      }
      declared += 1;
      if (!config.allowedLicenses.includes(pkg.license)) {
        incompatible.push(`${manifest}: ${pkg.license}`);
      }
    }
    if (incompatible.length > 0) {
      findings.push({
        severity: "warning",
        check: "license-compatibility",
        message: `Manifest license review needed: ${incompatible.slice(0, 6).join("; ")}${incompatible.length > 6 ? ` (+${incompatible.length - 6} more)` : ""}`,
        evidence: manifests.join(", "),
      });
    } else if (declared === 0) {
      findings.push({
        severity: "info",
        check: "license-compatibility",
        message: "No license fields to review in this tree shape",
        evidence: manifests.join(", "),
      });
    } else {
      findings.push({
        severity: "info",
        check: "license-compatibility",
        message: `All ${declared} workspace manifests declare allowlisted licenses`,
        evidence: manifests.join(", "),
      });
    }

    const lockPath = join(root, "pnpm-lock.yaml");
    if (fileExists(lockPath)) {
      // pnpm v9 layout: [lockfileVersion, settings, importers, packages, snapshots].
      // Only the `packages:` section is parsed (snapshots mirror it).
      const lines = readText(lockPath).split("\n");
      const versionsByName = new Map<string, Set<string>>();
      let inPackages = false;
      for (const line of lines) {
        if (line === "packages:") {
          inPackages = true;
          continue;
        }
        if (inPackages && /^[a-z]+:/.test(line)) {
          break;
        }
        if (!inPackages) {
          continue;
        }
        // Keys look like '[@scope/]name@version[(peer...)]:' at exactly
        // 2-space indent (deeper lines are importer/specifier blocks).
        const match = /^  (?! )'?(.+?)@([^@()\s:']+)(\(.*\))?'?:\s*$/.exec(
          line,
        );
        const name = match?.[1]?.replace(/\/$/, "");
        const version = match?.[2];
        if (
          name !== undefined &&
          version !== undefined &&
          name !== "" &&
          version !== ""
        ) {
          const set = versionsByName.get(name) ?? new Set<string>();
          set.add(version);
          versionsByName.set(name, set);
        }
      }
      const duplicated = [...versionsByName.entries()].filter(
        ([, v]) => v.size > 1,
      );
      if (duplicated.length > 0) {
        const shown = duplicated
          .slice(0, 10)
          .map(([n, v]) => `${n}(${[...v].join(",")})`)
          .join("; ");
        findings.push({
          severity: "warning",
          check: "duplicate-dependencies",
          message: `${duplicated.length} package(s) resolve to multiple versions${duplicated.length > 10 ? ` (showing 10)` : ""}: ${shown}`,
          evidence: "pnpm-lock.yaml",
        });
      } else {
        findings.push({
          severity: "info",
          check: "duplicate-dependencies",
          message: "No duplicated package versions in the lockfile",
          evidence: "pnpm-lock.yaml",
        });
      }
    }
    return findings;
  },
};
