// Proof of Awakening: end-to-end Batch 3 integration demo.
// Runs headlessly: Rust flow (supervisor delegation + browser mock +
// workspace isolation + real file writes + test run + context assembly +
// Tool-RAG + TypeCompact + memory recall + LLM route/meter + HITL approval +
// telemetry/cost + evidence verification) -> Void Neon topology tests.
// Exits non-zero on the first failing stage. Full-mock by default (CI-safe,
// deterministic, no network/display/keys); set TALOS_AWAKENING_LLM=real for
// a live model call (needs an OS-keychain key) and TALOS_BROWSER_LIVE=1 for
// live Playwright (needs the built browser worker).
//
// Report: docs/evidence/batch3-demo-report.json — steps, evidence, cost,
// duration, pass/fail.

import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const shell = process.platform === "win32";

// CLI flags (cross-platform env alternative): --llm-real enables a live
// model call (needs an OS-keychain key); --browser-real enables live
// Playwright (needs the built browser worker).
for (const arg of process.argv.slice(2)) {
  if (arg === "--llm-real") {
    process.env.TALOS_AWAKENING_LLM = "real";
  } else if (arg === "--browser-real") {
    process.env.TALOS_BROWSER_LIVE = "1";
  } else {
    console.log(`ignoring unknown flag ${arg}`);
  }
}

function sh(command, args, cwd, env) {
  try {
    const stdout = execFileSync(command, args, {
      cwd: cwd ?? root,
      shell,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
      timeout: 600_000,
      maxBuffer: 64 * 1024 * 1024,
      env: { ...process.env, ...(env ?? {}) },
    });
    return { ok: true, stdout };
  } catch (error) {
    return {
      ok: false,
      stdout: error.stdout?.toString() ?? "",
      stderr: error.stderr?.toString() ?? String(error),
    };
  }
}

const stages = [];
function stage(name, ok, detail) {
  stages.push({ name, ok, detail });
  console.log(`${ok ? "ok" : "FAIL"}  ${name}${detail ? ` — ${detail}` : ""}`);
  if (!ok) {
    finish(false);
  }
}

// Preflight: UI topology tests resolve `@talos/contracts` through its
// `dist/` build (gitignored). Fail fast with the fix instead of a
// cryptic vitest resolution error.
if (!existsSync(join(root, "packages", "contracts", "dist", "index.js"))) {
  stage(
    "preflight.contracts-dist",
    false,
    "packages/contracts/dist/index.js missing — run: pnpm --filter @talos/contracts build",
  );
}
stage("preflight.contracts-dist", true, "contracts dist present");

function finish(ok) {
  const report = {
    scenario: "proof-of-awakening",
    mode:
      process.env.TALOS_AWAKENING_LLM === "real"
        ? "llm-real"
        : process.env.TALOS_BROWSER_LIVE === "1"
          ? "browser-real"
          : "full-mock",
    ok,
    generatedAt: new Date().toISOString(),
    stages,
    flow: finish.flow ?? null,
    commit: process.env.TALOS_DEMO_COMMIT ?? null,
  };
  mkdirSync(join(root, "docs", "evidence"), { recursive: true });
  writeFileSync(
    join(root, "docs", "evidence", "batch3-demo-report.json"),
    `${JSON.stringify(report, null, 2)}\n`,
  );
  console.log(
    ok
      ? "proof-of-awakening: all stages passed"
      : "proof-of-awakening: FAILED (see docs/evidence/batch3-demo-report.json)",
  );
  process.exit(ok ? 0 : 1);
}

// Stage 1: headless Rust flow across every Batch 3 system.
const demo = sh(
  "cargo",
  ["run", "--quiet", "-p", "awakening_demo"],
  join(root, "crates"),
);
let flow = null;
try {
  flow = JSON.parse(demo.stdout.slice(demo.stdout.indexOf("{")));
} catch {
  flow = null;
}
if (!demo.ok || flow === null || flow.ok !== true) {
  stage("rust.flow", false, (demo.stderr || demo.stdout).slice(-2000));
}
finish.flow = flow;
const failedStep = (flow.steps ?? []).find((s) => !s.ok);
stage(
  "rust.flow",
  failedStep === undefined,
  failedStep === undefined
    ? `${flow.steps.length} steps ok in ${flow.metrics.totalMs}ms (mode ${flow.mode}, runner ${flow.metrics.testRunner}, session $${flow.metrics.sessionCostUsd}, ${flow.metrics.spansRecorded} spans, chain ${flow.metrics.chainChecked} links)`
    : `${failedStep.name}: ${failedStep.detail}`,
);

// Stage 2: Void Neon topology reflects delegation + tool + policy events.
const neon = sh("pnpm", [
  "--filter",
  "ui",
  "exec",
  "vitest",
  "run",
  "src/features/void-neon/",
]);
const neonDetail = neon.ok
  ? "topology suites green"
  : `${neon.stderr || ""}\n${neon.stdout || ""}`
      .replace(/\[[0-9;]*m/g, "")
      .slice(-1500);
stage("void-neon.topology", neon.ok, neonDetail);

finish(true);
