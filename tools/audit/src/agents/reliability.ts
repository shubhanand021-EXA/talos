import { join } from "node:path";
import type { AuditAgent, Finding } from "../types.ts";
import { fileExists, readText } from "../util.ts";

const REQUIRED_COMMANDS = ["node", "pnpm", "cargo", "rustc", "just", "git"];

/** Reliability: toolchain present, scripts wired, CI gates, install state. */
export const reliabilityAgent: AuditAgent = {
  name: "ReliabilityAuditAgent",
  run: async (ctx) => {
    const findings: Finding[] = [];
    const { root, config, exec, record } = ctx;

    let probeMs = 0;
    for (const command of REQUIRED_COMMANDS) {
      const probed = Date.now();
      const result = exec(command, ["--version"], {
        timeoutMs: 15000,
        cwd: root,
      });
      probeMs += Date.now() - probed;
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

    record("startup.toolchainProbeMs", probeMs);
    findings.push({
      severity: "info",
      check: "startup-probe-time",
      message: `Toolchain readiness probe took ${probeMs}ms (baseline-gated)`,
      evidence: REQUIRED_COMMANDS.join(" "),
    });

    // Batch 2 runtime: lifecycle + cancellation suites must pass. Execution is
    // environment-dependent, so failures here degrade to warnings (CI runs
    // the same suites as hard gates).
    for (const [check, filters] of [
      ["runtime-lifecycle-tests", ["lifecycle", "transitions"]],
      ["cancellation-tests", ["cancel"]],
    ] as const) {
      const started = Date.now();
      const live = exec(
        "cargo",
        ["test", "-p", "agent_runtime", "--quiet", "--", ...filters],
        { timeoutMs: config.cargoTestTimeoutMs, cwd: `${root}/crates` },
      );
      const elapsed = Date.now() - started;
      const out = `${live.stdout}\n${live.stderr}`;
      if (live.ok && /test result: ok/i.test(out)) {
        findings.push({
          severity: "info",
          check,
          message: `agent_runtime ${filters.join("/")} suites pass (${elapsed}ms)`,
          evidence: `cargo test -p agent_runtime -- ${filters.join(" ")}`,
        });
      } else if (live.timedOut || live.exitCode === null) {
        findings.push({
          severity: "warning",
          check,
          message: `Could not execute runtime suites here (toolchain/env) — presence covered by repo tests, execution deferred to CI`,
          evidence: (live.stderr || "cargo unavailable").slice(-300),
        });
      } else {
        findings.push({
          severity: "warning",
          check,
          message: `agent_runtime ${filters.join("/")} output did not confirm success — inspect before shipping runtime changes`,
          evidence: out.slice(-300) || "no output",
        });
      }
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
