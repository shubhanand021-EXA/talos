import { join } from "node:path";
import type { AuditAgent, Finding } from "../types.ts";
import { fileExists, grepFiles, readJson, readText } from "../util.ts";

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
    const { root, config, exec } = ctx;

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

    // Batch 2 safety defaults: process.exec and every computer-use tool ship
    // disabled; the live test suite is the authority (behavioral, not textual).
    const toolDefaults = exec(
      "cargo",
      [
        "test",
        "-p",
        "tool_runtime",
        "--quiet",
        "--",
        "exec_disabled_by_default",
        "computer_tools_disabled_by_default",
      ],
      { timeoutMs: config.cargoTestTimeoutMs, cwd: `${root}/crates` },
    );
    if (toolDefaults.ok) {
      findings.push({
        severity: "info",
        check: "tool-execution-defaults",
        message:
          "process.exec + all computer.* tools disabled by default (live registry tests pass)",
        evidence:
          "cargo test -p tool_runtime -- exec_disabled computer_tools_disabled",
      });
    } else if (toolDefaults.timedOut || toolDefaults.exitCode === null) {
      findings.push({
        severity: "warning",
        check: "tool-execution-defaults",
        message:
          "Could not execute tool default tests here (toolchain/env) — defaults unverified on this machine, enforced in CI",
        evidence: (toolDefaults.stderr || "cargo unavailable").slice(-300),
      });
    } else {
      findings.push({
        severity: "critical",
        check: "tool-execution-defaults",
        message:
          "Tool default-safety tests FAIL — an execution tool may ship enabled",
        evidence: (toolDefaults.stdout + toolDefaults.stderr).slice(-500),
      });
    }

    // Browser live mode must stay opt-in behind TALOS_BROWSER_LIVE=1.
    const liveGate = grepFiles(root, /TALOS_BROWSER_LIVE\s*===\s*["']1["']/, [
      "packages/browser-use/src",
    ]);
    const liveDisabledMsg = grepFiles(
      root,
      /disabled: set TALOS_BROWSER_LIVE=1/,
      ["packages/browser-use/src"],
    );
    if (liveGate.length > 0 && liveDisabledMsg.length > 0) {
      findings.push({
        severity: "info",
        check: "browser-live-disabled",
        message:
          "Live Chromium gated behind TALOS_BROWSER_LIVE=1 with a disabled-by-default message",
        evidence: liveGate
          .slice(0, 2)
          .map((h) => `${h.file}:${h.line}`)
          .join(", "),
      });
    } else {
      findings.push({
        severity: "critical",
        check: "browser-live-disabled",
        message:
          "Browser live-mode gate missing — live Chromium may not be opt-in",
        evidence: "packages/browser-use/src",
      });
    }

    // Real computer control must stay structurally impossible by default:
    // the driver compiles ONLY with the explicit `real-os-control` cargo
    // feature (never CI/default), and even then needs the envelope enabled,
    // a policy Allow verdict, and TALOS_COMPUTER_REAL=1 at runtime.
    const realStub = grepFiles(root, /DisabledRealComputer/, [
      "crates/computer_use/src",
    ]);
    const realGate = grepFiles(
      root,
      /#\[cfg\(feature = "real-os-control"\)\]/,
      ["crates/computer_use/src"],
    );
    const realOptIn = grepFiles(root, /TALOS_COMPUTER_REAL/, [
      "crates/computer_use/src",
    ]);
    // grepFiles walks directories only; the manifest is a single file.
    const manifestPath = join(root, "crates/computer_use/Cargo.toml");
    const realDepsOptional =
      fileExists(manifestPath) &&
      readText(manifestPath).includes("optional = true");
    if (
      realStub.length > 0 &&
      realGate.length > 0 &&
      realOptIn.length > 0 &&
      realDepsOptional
    ) {
      findings.push({
        severity: "info",
        check: "computer-real-disabled",
        message:
          "Real OS control is feature-gated (default builds ship only the disabled stub) with env opt-in",
        evidence: realGate
          .slice(0, 2)
          .map((h) => `${h.file}:${h.line}`)
          .join(", "),
      });
    } else {
      findings.push({
        severity: "critical",
        check: "computer-real-disabled",
        message:
          "Computer-use real-adapter guard missing — OS control may be reachable",
        evidence: "crates/computer_use/src",
      });
    }

    // Dangerous capabilities must route through human review.
    const reviewGate = grepFiles(root, /RequireHumanReview/, [
      "crates/policy_broker/src",
    ]);
    if (reviewGate.length > 0) {
      findings.push({
        severity: "info",
        check: "dangerous-capabilities-gated",
        message: `High-risk actions escalate to human review (${reviewGate.length} gate reference(s))`,
        evidence: reviewGate
          .slice(0, 2)
          .map((h) => `${h.file}:${h.line}`)
          .join(", "),
      });
    } else {
      findings.push({
        severity: "critical",
        check: "dangerous-capabilities-gated",
        message: "No human-review gate in the policy broker",
        evidence: "crates/policy_broker/src",
      });
    }
    return findings;
  },
};
