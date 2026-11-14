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

/** Exact Tauri IPC origin (CodeQL js/incomplete-url-substring-sanitization
 * #9): a `startsWith("http://ipc.localhost")` prefix test wrongly trusts
 * `http://ipc.localhost.evil.com`. Compare the parsed hostname instead. */
function isLocalIpcOrigin(host: string): boolean {
  try {
    const url = new URL(host);
    return url.protocol === "http:" && url.hostname === "ipc.localhost";
  } catch {
    return false;
  }
}

/** Security: unsafe Rust, Tauri capabilities, CSP, secrets, shady deps. */
export const securityAgent: AuditAgent = {
  name: "SecurityAuditAgent",
  run: async (ctx) => {
    const findings: Finding[] = [];
    const { root, config, exec } = ctx;

    const unsafeHits = grepFiles(root, /\bunsafe\b/, ["crates"]);
    // Scoped exception (ADR 0010): Win32 Job Object FFI in
    // crates/sandbox/src/windows.rs is inherently `unsafe fn` in the
    // `windows` crate — there is no safe wrapper. Hits there are permitted
    // only when every one carries a nearby SAFETY justification, plus the
    // single `unsafe_code` lint-scope line in the crate's Cargo.toml (which
    // documents the scope). Everything else still fails closed.
    const sandboxSrc = "crates/sandbox/src/windows.rs";
    const sandboxManifest = "crates/sandbox/Cargo.toml";
    const otherUnsafe = unsafeHits.filter(
      (hit) => hit.file !== sandboxSrc && hit.file !== sandboxManifest,
    );
    const sandboxSrcHits = unsafeHits.filter((hit) => hit.file === sandboxSrc);
    const sandboxManifestHits = unsafeHits.filter(
      (hit) => hit.file === sandboxManifest && hit.text.includes("unsafe_code"),
    );
    const sandboxManifestOther = unsafeHits.filter(
      (hit) =>
        hit.file === sandboxManifest && !hit.text.includes("unsafe_code"),
    );
    let sandboxUnjustified: typeof unsafeHits = [];
    if (sandboxSrcHits.length > 0) {
      try {
        const lines = readText(join(root, sandboxSrc)).split("\n");
        sandboxUnjustified = sandboxSrcHits.filter((hit) => {
          const from = Math.max(0, hit.line - 6);
          const context = lines.slice(from, hit.line).join("\n");
          return !context.includes("SAFETY");
        });
      } catch {
        sandboxUnjustified = sandboxSrcHits;
      }
    }
    const deniedUnsafe = [
      ...otherUnsafe,
      ...sandboxManifestOther,
      ...sandboxUnjustified,
    ];
    if (deniedUnsafe.length > 0) {
      for (const hit of deniedUnsafe.slice(0, 10)) {
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
        message:
          sandboxSrcHits.length > 0
            ? `No unjustified \`unsafe\` blocks in crates/ (scoped Win32 FFI in ${sandboxSrc} carries SAFETY justifications per ADR 0010)`
            : "No `unsafe` blocks in crates/",
        evidence:
          sandboxSrcHits.length > 0
            ? `grep \\bunsafe\\b crates/ (${sandboxSrcHits.length} scoped hits + ${sandboxManifestHits.length} lint-scope hits, all justified)`
            : "grep \\bunsafe\\b crates/",
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
      const remote = hosts.filter((h) => !isLocalIpcOrigin(h));
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

    // At-rest encryption must be wired, not just documented: envelope module
    // present, keychain key referenced, encrypted open on the production
    // startup path, sealed backups. (sqlx 0.8 has no SQLCipher feature, so
    // there is no PRAGMA key to check — the envelope code paths ARE the
    // check, per the Batch 4 design.)
    const envelopeHits = grepFiles(root, /TALOSENV/, [
      "crates/evidence_store/src",
    ]);
    const keyHits = grepFiles(root, /database\.encryption_key/, [
      "crates/evidence_store/src",
      "apps/desktop/src-tauri/src",
    ]);
    const openHits = grepFiles(root, /connect_encrypted_file/, [
      "crates/evidence_store/src",
      "apps/desktop/src-tauri/src",
    ]);
    const missingEncryption = [
      envelopeHits.length === 0 ? "envelope magic (TALOSENV)" : null,
      keyHits.length === 0 ? "keychain key (database.encryption_key)" : null,
      openHits.length === 0 ? "encrypted open (connect_encrypted_file)" : null,
    ].filter((s): s is string => s !== null);
    if (missingEncryption.length > 0) {
      findings.push({
        severity: "critical",
        check: "encryption-at-rest-wired",
        message: `At-rest encryption wiring missing: ${missingEncryption.join(", ")}`,
        evidence: "crates/evidence_store/src, apps/desktop/src-tauri/src",
      });
    } else {
      findings.push({
        severity: "info",
        check: "encryption-at-rest-wired",
        message:
          "Envelope encryption wired: magic, keychain key, and encrypted open on the startup path",
        evidence: [
          ...envelopeHits.slice(0, 1),
          ...keyHits.slice(0, 1),
          ...openHits.slice(0, 1),
        ]
          .map((h) => `${h.file}:${h.line}`)
          .join(", "),
      });
    }

    // Signed updater must be configured: pinned pubkey + endpoint in
    // tauri.conf.json, ed25519 signing tooling, backend IPC commands.
    const updaterConf = join(root, "apps/desktop/src-tauri/tauri.conf.json");
    let updaterConfOk = false;
    if (fileExists(updaterConf)) {
      try {
        const conf = readJson<{
          plugins?: { updater?: { pubkey?: string; endpoints?: unknown[] } };
        }>(updaterConf);
        updaterConfOk =
          typeof conf.plugins?.updater?.pubkey === "string" &&
          conf.plugins.updater.pubkey.length >= 32 &&
          Array.isArray(conf.plugins?.updater?.endpoints) &&
          (conf.plugins?.updater?.endpoints?.length ?? 0) > 0;
      } catch {
        updaterConfOk = false;
      }
    }
    const signScript = fileExists(join(root, "scripts/sign-release.mjs"));
    const updaterCmds = grepFiles(root, /check_for_updates/, [
      "apps/desktop/src-tauri/src",
    ]);
    const missingUpdater = [
      !updaterConfOk
        ? "tauri.conf.json plugins.updater (pubkey + endpoints)"
        : null,
      !signScript ? "scripts/sign-release.mjs" : null,
      updaterCmds.length === 0
        ? "check_for_updates/install_update commands"
        : null,
    ].filter((s): s is string => s !== null);
    if (missingUpdater.length > 0) {
      findings.push({
        severity: "critical",
        check: "signed-updater-configured",
        message: `Signed updater incomplete: ${missingUpdater.join(", ")}`,
        evidence:
          "apps/desktop/src-tauri/tauri.conf.json, scripts/sign-release.mjs",
      });
    } else {
      findings.push({
        severity: "info",
        check: "signed-updater-configured",
        message:
          "Signed updater configured: pinned pubkey + endpoint, signing script, backend commands",
        evidence: "tauri.conf.json:plugins.updater, scripts/sign-release.mjs",
      });
    }
    return findings;
  },
};
