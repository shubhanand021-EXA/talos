import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, it } from "node:test";
import { architectureAgent } from "../src/agents/architecture.ts";
import { privacyAgent } from "../src/agents/privacy.ts";
import { productAgent } from "../src/agents/product.ts";
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
      stdout: "",
      stderr: "",
      exitCode: 0,
      timedOut: false,
    }),
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
    "use policy_broker::DefaultPolicyEngine;\n",
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
        'fetch("https://telemetry.example.com/x");\n',
      );
      const remote = await privacyAgent.run(
        ctx(root, { telemetryDeps: [], telemetryPatterns: [] }),
      );
      assert.ok(
        remote.some(
          (f) => f.severity === "warning" && f.check === "no-remote-endpoints",
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
});
