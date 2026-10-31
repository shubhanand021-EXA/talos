import { join } from "node:path";
import type { AuditAgent, Finding } from "../types.ts";
import { fileExists, readText } from "../util.ts";

const REQUIRED_COMMANDS = ["node", "pnpm", "cargo", "rustc", "just", "git"];

/** Reliability: toolchain present, scripts wired, CI gates, install state. */
export const reliabilityAgent: AuditAgent = {
  name: "ReliabilityAuditAgent",
  run: async (ctx) => {
    const findings: Finding[] = [];
    const { root, config, exec } = ctx;

    for (const command of REQUIRED_COMMANDS) {
      const result = exec(command, ["--version"], {
        timeoutMs: 15000,
        cwd: root,
      });
      if (result.ok) {
        findings.push({
          severity: "info",
          check: "toolchain-present",
          message: `${command} resolvable (${result.stdout.trim().split("\n")[0]?.slice(0, 80) ?? "ok"})`,
          evidence: `${command} --version`,
        });
      } else {
        findings.push({
          severity: "warning",
          check: "toolchain-present",
          message: `${command} not resolvable — gates requiring it are unverifiable here`,
          evidence: (result.stderr || "missing binary").slice(0, 200),
        });
      }
    }

    const pkgPath = join(root, "package.json");
    const pkg = JSON.parse(readText(pkgPath)) as {
      scripts?: Record<string, string>;
    };
    const scripts = pkg.scripts ?? {};
    for (const script of config.requiredScripts) {
      if (typeof scripts[script] === "string") {
        findings.push({
          severity: "info",
          check: "scripts-wired",
          message: `Root script "${script}" present`,
          evidence: `package.json:scripts.${script}`,
        });
      } else {
        findings.push({
          severity: "critical",
          check: "scripts-wired",
          message: `Root script "${script}" missing — automation entry point broken`,
          evidence: "package.json",
        });
      }
    }
    const uiPkgPath = join(root, "apps/ui/package.json");
    if (fileExists(uiPkgPath)) {
      const ui = JSON.parse(readText(uiPkgPath)) as {
        scripts?: Record<string, string>;
      };
      for (const script of config.requiredUiScripts) {
        if (typeof (ui.scripts ?? {})[script] !== "string") {
          findings.push({
            severity: "critical",
            check: "scripts-wired",
            message: `UI script "${script}" missing`,
            evidence: "apps/ui/package.json",
          });
        }
      }
    }

    for (const lock of [
      "pnpm-lock.yaml",
      "crates/Cargo.lock",
      "apps/desktop/src-tauri/Cargo.lock",
    ]) {
      if (!fileExists(join(root, lock))) {
        findings.push({
          severity: "warning",
          check: "lockfiles-present",
          message: `Lockfile missing: ${lock} (reproducible installs at risk)`,
          evidence: lock,
        });
      }
    }
    if (
      !findings.some(
        (f) => f.check === "lockfiles-present" && f.severity !== "info",
      )
    ) {
      findings.push({
        severity: "info",
        check: "lockfiles-present",
        message: "All workspace lockfiles present",
        evidence: "pnpm-lock.yaml + 2 Cargo.lock",
      });
    }

    if (!fileExists(join(root, "node_modules"))) {
      findings.push({
        severity: "warning",
        check: "deps-installed",
        message: "node_modules missing — run pnpm install before deeper gates",
        evidence: "node_modules/",
      });
    }

    const ciPath = join(root, ".github/workflows/ci.yml");
    if (!fileExists(ciPath)) {
      findings.push({
        severity: "critical",
        check: "ci-gates",
        message: "CI workflow missing",
        evidence: ".github/workflows/ci.yml",
      });
    } else {
      const ci = readText(ciPath);
      for (const gate of ["typecheck", "clippy", "cargo test", "audit"]) {
        if (!ci.includes(gate)) {
          findings.push({
            severity: "warning",
            check: "ci-gates",
            message: `CI workflow does not mention "${gate}"`,
            evidence: ".github/workflows/ci.yml",
          });
        }
      }
      if (!findings.some((f) => f.check === "ci-gates")) {
        findings.push({
          severity: "info",
          check: "ci-gates",
          message:
            "CI workflow covers typecheck, clippy, cargo test, and audit",
          evidence: ".github/workflows/ci.yml",
        });
      }
    }
    return findings;
  },
};
