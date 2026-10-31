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

    // Tools must never bypass the gated executor: no direct adapter calls in
    // the shell, and invocations flow through tool_runtime::invoke.
    const executorPath = join(root, "crates/tool_runtime/src/executor.rs");
    if (!fileExists(executorPath)) {
      findings.push({
        severity: "info",
        check: "tools-through-runtime",
        message: "Tool runtime absent here — invocation path unverifiable",
        evidence: "crates/tool_runtime/src",
      });
    } else {
      const directAdapter = grepFiles(
        root,
        /tools_fs::|tools_browser::|MockComputer::|MockBrowserAdapter::/,
        ["apps/desktop/src-tauri/src"],
      );
      const gatedInvoke = grepFiles(root, /tool_runtime::invoke/, [
        "apps/desktop/src-tauri/src",
      ]);
      if (directAdapter.length > 0) {
        findings.push({
          severity: "critical",
          check: "tools-through-runtime",
          message: `Direct tool-adapter call in the shell bypasses policy gating: ${directAdapter[0]?.file}:${directAdapter[0]?.line}`,
          evidence: `${directAdapter[0]?.file}:${directAdapter[0]?.line}`,
        });
      } else if (gatedInvoke.length === 0) {
        findings.push({
          severity: "critical",
          check: "tools-through-runtime",
          message:
            "No tool_runtime::invoke call in the shell — tools bypass the gated executor",
          evidence: "apps/desktop/src-tauri/src",
        });
      } else {
        findings.push({
          severity: "info",
          check: "tools-through-runtime",
          message: `All shell tool calls flow through the gated executor (${gatedInvoke.length} call site(s))`,
          evidence: gatedInvoke
            .slice(0, 2)
            .map((h) => `${h.file}:${h.line}`)
            .join(", "),
        });
      }
    }

    // Browser + computer actions must route through policy capabilities.
    const openBrowser = grepFiles(root, /OpenBrowser/, [
      "crates/policy_broker/src",
    ]);
    const browserArms = grepFiles(root, /tools_browser/, [
      "crates/tool_runtime/src",
    ]);
    if (
      !fileExists(join(root, "crates/tool_runtime/src")) ||
      !fileExists(join(root, "crates/policy_broker/src"))
    ) {
      findings.push({
        severity: "info",
        check: "browser-through-policy",
        message: "Browser policy path unverifiable in this tree shape",
        evidence: "crates/tool_runtime/src",
      });
    } else if (openBrowser.length === 0 || browserArms.length === 0) {
      findings.push({
        severity: "critical",
        check: "browser-through-policy",
        message: "Browser actions lack a policy capability or gated arms",
        evidence: "crates/tool_runtime/src, crates/policy_broker/src",
      });
    } else {
      findings.push({
        severity: "info",
        check: "browser-through-policy",
        message: `Browser actions gated via OpenBrowser (${browserArms.length} executor arm(s))`,
        evidence: browserArms
          .slice(0, 2)
          .map((h) => `${h.file}:${h.line}`)
          .join(", "),
      });
    }

    const uiAction = grepFiles(root, /UiAction/, ["crates/policy_broker/src"]);
    const computerArms = grepFiles(
      root,
      /computer::MockComputer|MockComputer::/,
      ["crates/tool_runtime/src"],
    );
    if (
      !fileExists(join(root, "crates/tool_runtime/src")) ||
      !fileExists(join(root, "crates/computer_use/src"))
    ) {
      findings.push({
        severity: "info",
        check: "computer-through-policy",
        message: "Computer-use policy path unverifiable in this tree shape",
        evidence: "crates/computer_use/src",
      });
    } else if (uiAction.length === 0 || computerArms.length === 0) {
      findings.push({
        severity: "critical",
        check: "computer-through-policy",
        message: "Computer actions lack a policy capability or gated arms",
        evidence: "crates/tool_runtime/src, crates/computer_use/src",
      });
    } else {
      findings.push({
        severity: "info",
        check: "computer-through-policy",
        message: `Computer actions gated via UiAction safety envelope (${computerArms.length} executor arm(s))`,
        evidence: computerArms
          .slice(0, 2)
          .map((h) => `${h.file}:${h.line}`)
          .join(", "),
      });
    }

    // Tool-RAG is read-only: ranking must never execute tools.
    if (!fileExists(join(root, "crates/tool_rag/src"))) {
      findings.push({
        severity: "info",
        check: "rag-no-execution",
        message: "Tool-RAG crate absent here — execution-freedom unverifiable",
        evidence: "crates/tool_rag/src",
      });
    } else {
      const ragInvoke = grepFiles(root, /\binvoke\s*\(/, [
        "crates/tool_rag/src",
      ]);
      if (ragInvoke.length > 0) {
        findings.push({
          severity: "critical",
          check: "rag-no-execution",
          message: `Tool-RAG ranking path executes tools: ${ragInvoke[0]?.file}:${ragInvoke[0]?.line}`,
          evidence: `${ragInvoke[0]?.file}:${ragInvoke[0]?.line}`,
        });
      } else {
        findings.push({
          severity: "info",
          check: "rag-no-execution",
          message: "Tool-RAG ranking is read-only — no execution path",
          evidence: "crates/tool_rag/src",
        });
      }
    }
    return findings;
  },
};
