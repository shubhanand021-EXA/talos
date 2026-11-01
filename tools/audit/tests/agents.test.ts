import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, it } from "node:test";
import { architectureAgent } from "../src/agents/architecture.ts";
import { evidenceAgent } from "../src/agents/evidence.ts";
import { performanceAgent } from "../src/agents/performance.ts";
import { privacyAgent } from "../src/agents/privacy.ts";
import { productAgent } from "../src/agents/product.ts";
import { reliabilityAgent } from "../src/agents/reliability.ts";
import { securityAgent } from "../src/agents/security.ts";
import { uiuxAgent } from "../src/agents/uiux.ts";
import { DEFAULT_CONFIG } from "../src/config.ts";
import type { AuditContext } from "../src/types.ts";

function fixture(): string {
  return mkdtempSync(join(tmpdir(), "talos-audit-agents-"));
}

function write(root: string, rel: string, content: string): void {
  const parts = rel.split("/");
  const dir = join(root, ...parts.slice(0, -1));
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(root, ...parts), content);
}

function ctx(
  root: string,
  overrides: Partial<AuditContext["config"]> = {},
): AuditContext {
  return {
    root,
    config: { ...DEFAULT_CONFIG, ...overrides },
    exec: () => ({
      ok: true,
      stdout: "test result: ok. 1 passed; 0 failed",
      stderr: "",
      exitCode: 0,
      timedOut: false,
    }),
    record: () => {},
  };
}

/** Minimal healthy repo exercising every agent's pass path. */
function healthyRepo(): string {
  const root = fixture();
  write(root, "package.json", JSON.stringify({ scripts: { test: "x" } }));
  write(
    root,
    "apps/desktop/src-tauri/capabilities/default.json",
    JSON.stringify({ windows: ["main"], permissions: [] }),
  );
  write(
    root,
    "apps/desktop/src-tauri/tauri.conf.json",
    JSON.stringify({ app: { security: { csp: "default-src 'self'" } } }),
  );
  write(root, "apps/desktop/src-tauri/src/main.rs", "fn main() {}\n");
  write(
    root,
    "apps/ui/src/app/router.tsx",
    'createHashRouter([{ path: "/", children: [{ index: true }, { path: "agents" }, { path: "evidence" }, { path: "audit" }, { path: "settings" }] }]);\n',
  );
  write(
    root,
    "apps/ui/src/design-system/GlassPanel.tsx",
    "export function GlassPanel() { return null; }\n",
  );
  write(
    root,
    "apps/ui/src/features/dashboard/DashboardPage.tsx",
    `import { SectionHeader } from "../../design-system/index.js";\nexport function DashboardPage() { return null; }\n${"x".repeat(600)}\n`,
  );
  write(
    root,
    "apps/ui/src/bridge/client.ts",
    'import { invoke } from "@tauri-apps/api/core";\n',
  );
  write(
    root,
    "packages/contracts/package.json",
    JSON.stringify({ name: "@talos/contracts" }),
  );
  write(
    root,
    "crates/policy_broker/src/lib.rs",
    "pub struct DefaultPolicyEngine;\n",
  );
  write(
    root,
    "crates/policy_broker/src/engine.rs",
    "// High-risk actions escalate to human review.\n// RequireHumanReview gates irreversible tool calls.\n// OpenBrowser and UiAction capabilities route browser/computer actions.\n",
  );
  write(
    root,
    "crates/tool_runtime/src/executor.rs",
    "// Receipts persist as tool_receipt evidence with evidence_id linkage.\n// Browser arms use tools_browser; computer arms use computer::MockComputer.\n",
  );
  write(
    root,
    "crates/context_budget/src/compact.rs",
    "// Compaction receipts persist as context_compaction evidence with proof_hash.\n",
  );
  for (const migration of [
    "0001_init.sql",
    "0002_ledger.sql",
    "0003_runtime.sql",
  ]) {
    write(root, `crates/evidence_store/migrations/${migration}`, "-- init\n");
  }
  write(
    root,
    "packages/browser-use/src/live.ts",
    'export const liveGate = process.env.TALOS_BROWSER_LIVE === "1";\n// disabled: set TALOS_BROWSER_LIVE=1 to opt into live Chromium\n',
  );
  write(
    root,
    "packages/browser-use/src/session.ts",
    "// Isolated browser session: fresh profile dir per task, never personal.\n",
  );
  write(
    root,
    "crates/computer_use/src/real.rs",
    '#[cfg(feature = "real-os-control")]\n// TALOS_COMPUTER_REAL=1 runtime opt-in.\n// Always-disabled stub: DisabledRealComputer fails closed.\n',
  );
  write(
    root,
    "crates/computer_use/Cargo.toml",
    '[features]\nreal-os-control = []\n[dependencies]\nenigo = { version = "0.6", optional = true }\n',
  );
  write(
    root,
    "apps/ui/src/features/computer/ComputerPage.tsx",
    `export function ComputerPage() { return null; }\n<button>Emergency stop</button>\n${"x".repeat(600)}\n`,
  );
  write(
    root,
    "apps/ui/src/app/chrome/BottomStatusBar.tsx",
    "export function BottomStatusBar() { return 'computer-use STOPPED'; }\n",
  );
  write(root, "crates/talos_common/src/lib.rs", "// common\n");
  write(root, "crates/agent_runtime/src/lib.rs", "// runtime\n");
  write(
    root,
    "tools/audit/package.json",
    JSON.stringify({ name: "@talos/audit-swarm" }),
  );
  write(
    root,
    "apps/desktop/src-tauri/src/commands.rs",
    "use policy_broker::DefaultPolicyEngine;\n// All tool calls flow through tool_runtime::invoke.\n// TypeCompact receipts persist as context_compaction evidence.\n",
  );
  write(
    root,
    "crates/evidence_store/src/ledger.rs",
    "pub fn verify_chain() {}\n// tamper test\n",
  );
  write(root, "README.md", "Talos agent desktop evidence policy\n");
  write(root, "docs/PROGRESS.md", `## 2026-10-31 — log\n${"x".repeat(300)}\n`);
  write(root, "docs/EVIDENCE.md", `## 2026-10-31 — log\n${"x".repeat(300)}\n`);
  write(root, "docs/ARCHITECTURE.md", `Talos\n${"x".repeat(300)}\n`);
  write(root, "docs/ROADMAP.md", `Talos\n${"x".repeat(300)}\n`);
  return root;
}

describe("audit agents", () => {
  it("security passes a clean tree and flags secrets + unsafe", async () => {
    const root = healthyRepo();
    try {
      const clean = await securityAgent.run(
        ctx(root, {
          forbiddenPatterns: ["AKIA[0-9A-Z]{16}"],
          suspiciousDependencies: ["event-stream"],
        }),
      );
      assert.ok(!clean.some((f) => f.severity === "critical"));

      write(root, "leak.txt", "key = AKIAIOSFODNN7EXAMPLE\n");
      const dirty = await securityAgent.run(
        ctx(root, {
          forbiddenPatterns: ["AKIA[0-9A-Z]{16}"],
          suspiciousDependencies: [],
        }),
      );
      assert.ok(
        dirty.some(
          (f) => f.severity === "critical" && f.check === "no-secrets",
        ),
      );

      write(
        root,
        "crates/evil/src/lib.rs",
        "pub fn f() { unsafe { core::ptr::null::<u8>(); } }\n",
      );
      const unsafeRun = await securityAgent.run(
        ctx(root, { forbiddenPatterns: [] }),
      );
      assert.ok(
        unsafeRun.some(
          (f) => f.severity === "critical" && f.check === "no-unsafe-rust",
        ),
      );
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });

  it("privacy flags telemetry SDKs and remote endpoints", async () => {
    const root = healthyRepo();
    try {
      const clean = await privacyAgent.run(ctx(root));
      assert.ok(clean.every((f) => f.severity !== "critical"));

      write(
        root,
        "apps/ui/package.json",
        JSON.stringify({ dependencies: { "posthog-js": "^1.0.0" } }),
      );
      const dirty = await privacyAgent.run(
        ctx(root, { telemetryDeps: ["posthog-js"], telemetryPatterns: [] }),
      );
      assert.ok(
        dirty.some(
          (f) => f.severity === "critical" && f.check === "no-analytics-sdk",
        ),
      );

      write(
        root,
        "apps/ui/src/ping.ts",
        'fetch("https://telemetry-collector.dev/x");\n',
      );
      const remote = await privacyAgent.run(
        ctx(root, { telemetryDeps: [], telemetryPatterns: [] }),
      );
      assert.ok(
        remote.some(
          (f) => f.severity === "warning" && f.check === "no-remote-endpoints",
        ),
      );

      write(
        root,
        "apps/ui/src/docs.ts",
        'const a = "https://example.com/x";\nconst b = "https://other.example/x";\n',
      );
      const reserved = await privacyAgent.run(
        ctx(root, { telemetryDeps: [], telemetryPatterns: [] }),
      );
      assert.ok(
        reserved.every(
          (f) =>
            !(
              f.check === "no-remote-endpoints" &&
              /example/.test(f.evidence as string)
            ),
        ),
      );
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });

  it("architecture enforces layout, contracts, and UI/backend separation", async () => {
    const root = healthyRepo();
    try {
      const config = {
        requiredRoutes: ["/", "/agents"],
        requiredComponents: ["GlassPanel"],
      };
      const clean = await architectureAgent.run(ctx(root, config));
      assert.ok(clean.every((f) => f.severity !== "critical"));

      write(
        root,
        "apps/ui/src/leak.ts",
        'import { x } from "policy_broker";\n',
      );
      const leaked = await architectureAgent.run(ctx(root, config));
      assert.ok(
        leaked.some(
          (f) => f.severity === "critical" && f.check === "no-backend-in-ui",
        ),
      );
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });

  it("security gates tool, browser, computer, and policy defaults", async () => {
    const root = healthyRepo();
    try {
      const clean = await securityAgent.run(ctx(root));
      assert.ok(!clean.some((f) => f.severity === "critical"));
      for (const check of [
        "tool-execution-defaults",
        "browser-live-disabled",
        "computer-real-disabled",
        "dangerous-capabilities-gated",
      ]) {
        assert.ok(
          clean.some((f) => f.check === check && f.severity === "info"),
          check,
        );
      }

      write(
        root,
        "packages/browser-use/src/live.ts",
        "export const liveGate = true;\n",
      );
      const open = await securityAgent.run(ctx(root));
      assert.ok(
        open.some(
          (f) =>
            f.severity === "critical" && f.check === "browser-live-disabled",
        ),
      );
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });

  it("reliability runs lifecycle and cancellation suites", async () => {
    const root = healthyRepo();
    try {
      const recorded: Record<string, number> = {};
      const context = {
        ...ctx(root),
        record: (metric: string, value: number) => {
          recorded[metric] = value;
        },
      };
      const results = await reliabilityAgent.run(context);
      assert.ok(
        results.some(
          (f) => f.check === "runtime-lifecycle-tests" && f.severity === "info",
        ),
      );
      assert.ok(
        results.some(
          (f) => f.check === "cancellation-tests" && f.severity === "info",
        ),
      );
      assert.ok(typeof recorded["startup.toolchainProbeMs"] === "number");
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });

  it("evidence verifies receipts, migrations, and compaction wiring", async () => {
    const root = healthyRepo();
    try {
      const recorded: Record<string, number> = {};
      const context = {
        ...ctx(root),
        record: (metric: string, value: number) => {
          recorded[metric] = value;
        },
      };
      const results = await evidenceAgent.run(context);
      assert.ok(!results.some((f) => f.severity === "critical"));
      for (const check of [
        "migrations-present",
        "tool-receipts-linked",
        "compaction-receipts-preserved",
      ]) {
        assert.ok(
          results.some((f) => f.check === check && f.severity === "info"),
          check,
        );
      }
      assert.ok(typeof recorded["rust.evidenceSubsetMs"] === "number");
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });

  it("performance budgets dist and records chunk metrics", async () => {
    const root = healthyRepo();
    try {
      write(root, "apps/ui/dist/index.html", "<html></html>");
      write(
        root,
        "apps/ui/dist/assets/main-abc123.js",
        `console.log("main");\n${"x".repeat(2000)}\n`,
      );
      write(
        root,
        "apps/ui/dist/assets/void-neon-xyz.js",
        `console.log("neon");\n${"y".repeat(1000)}\n`,
      );
      const recorded: Record<string, number> = {};
      const context = {
        ...ctx(root),
        record: (metric: string, value: number) => {
          recorded[metric] = value;
        },
      };
      const results = await performanceAgent.run(context);
      assert.ok(!results.some((f) => f.severity === "critical"));
      assert.ok(
        results.some(
          (f) => f.check === "void-neon-chunk-gzip" && f.severity === "info",
        ),
      );
      assert.ok((recorded["ui.distBytes"] ?? 0) > 0);
      assert.ok((recorded["ui.mainJsGzipBytes"] ?? 0) > 0);
      assert.ok((recorded["ui.voidNeonChunkGzipBytes"] ?? 0) > 0);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });

  it("product tracks features, objectives, docs, and snapshot sync", async () => {
    const root = healthyRepo();
    try {
      const results = await productAgent.run(
        ctx(root, {
          featureKeywords: ["Tool-RAG", "TypeCompact"],
          forbiddenTopLevelDirs: [],
        }),
      );
      assert.ok(
        results.some(
          (f) =>
            f.check === "readme-matches-features" && f.severity === "warning",
        ),
      );
      assert.ok(
        results.some(
          (f) => f.check === "batch2-objectives" && f.severity === "warning",
        ),
      );
      assert.ok(
        results.some(
          (f) => f.check === "docs-updated" && f.severity === "warning",
        ),
      );
      assert.ok(
        results.some(
          (f) =>
            f.check === "ui-baseline-snapshot-in-sync" && f.severity === "info",
        ),
      );
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });

  it("architecture blocks runtime bypasses and rag execution", async () => {
    const root = healthyRepo();
    try {
      const clean = await architectureAgent.run(ctx(root));
      assert.ok(!clean.some((f) => f.severity === "critical"));
      for (const check of [
        "tools-through-runtime",
        "browser-through-policy",
        "computer-through-policy",
        "rag-no-execution",
      ]) {
        assert.ok(
          clean.some((f) => f.check === check && f.severity === "info"),
          check,
        );
      }

      write(
        root,
        "apps/desktop/src-tauri/src/bridge.rs",
        'pub fn leak() { let _ = crate::tools_fs::read("x"); }\n',
      );
      const bypassed = await architectureAgent.run(ctx(root));
      assert.ok(
        bypassed.some(
          (f) =>
            f.severity === "critical" && f.check === "tools-through-runtime",
        ),
      );
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });

  it("privacy documents browser profile isolation", async () => {
    const root = healthyRepo();
    try {
      const clean = await privacyAgent.run(ctx(root));
      assert.ok(
        clean.some(
          (f) =>
            f.check === "browser-profile-isolation" && f.severity === "info",
        ),
      );
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });

  it("uiux requires routes, components, and rejects placeholders", async () => {
    const root = healthyRepo();
    try {
      const config = {
        requiredRoutes: ["/", "/agents"],
        requiredComponents: ["GlassPanel"],
      };
      const clean = await uiuxAgent.run(ctx(root, config));
      assert.ok(!clean.some((f) => f.severity === "critical"));

      write(
        root,
        "apps/ui/src/features/blank/BlankPage.tsx",
        "export function BlankPage() { return null; } // lorem ipsum dolor\n",
      );
      const dirty = await uiuxAgent.run(ctx(root, config));
      assert.ok(dirty.some((f) => f.check === "no-placeholder-text"));
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });

  it("product checks vision, docs, artifacts, and drift", async () => {
    const root = healthyRepo();
    try {
      const clean = await productAgent.run(
        ctx(root, { forbiddenTopLevelDirs: [] }),
      );
      assert.ok(
        !clean.some(
          (f) => f.check === "vision-readme" && f.severity !== "info",
        ),
      );
      assert.ok(
        clean.some(
          (f) => f.check === "docs-maintained" && f.severity === "info",
        ),
      );

      mkdirSync(join(root, "apps", "mobile"), { recursive: true });
      const drifted = await productAgent.run(
        ctx(root, { forbiddenTopLevelDirs: ["apps/mobile"] }),
      );
      assert.ok(
        drifted.some(
          (f) => f.check === "no-scope-drift" && f.severity === "warning",
        ),
      );
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });

  it("uiux surfaces critical actions, e-stop, and state affordances", async () => {
    const root = healthyRepo();
    try {
      const clean = await uiuxAgent.run(
        ctx(root, { requiredRoutes: ["/", "/agents"] }),
      );
      assert.ok(!clean.some((f) => f.severity === "critical"));
      assert.ok(
        clean.some(
          (f) => f.check === "emergency-stop-visible" && f.severity === "info",
        ),
      );
      write(
        root,
        "apps/ui/src/features/blank/AsyncBlankPage.tsx",
        'import { useEffect } from "react";\nexport function AsyncBlankPage() { useEffect(() => {}, []); return null; }\n',
      );
      const dirty = await uiuxAgent.run(
        ctx(root, { requiredRoutes: ["/", "/agents"] }),
      );
      assert.ok(
        dirty.some(
          (f) =>
            f.check === "empty-loading-states" &&
            f.severity === "warning" &&
            f.evidence.includes("AsyncBlankPage"),
        ),
      );
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });
});
