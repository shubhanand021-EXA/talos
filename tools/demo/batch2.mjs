// Batch 2 integration demo: deterministic end-to-end exercise.
// Runs headlessly: Rust flow (runtime + tools + browser/computer mocks +
// context + RAG + evidence + compensation) -> Void Neon topology tests ->
// audit swarm -> structured report in docs/evidence/batch2-demo-latest.json.
// Exits non-zero on the first failing stage. No network, no display.

import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const shell = process.platform === "win32";

function sh(command, args, cwd) {
  try {
    const stdout = execFileSync(command, args, {
      cwd: cwd ?? root,
      shell,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
      timeout: 600_000,
      maxBuffer: 64 * 1024 * 1024,
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

function finish(ok) {
  const report = {
    scenario: "batch2-integration-demo",
    ok,
    generatedAt: new Date().toISOString(),
    stages,
    commit: process.env.TALOS_DEMO_COMMIT ?? null,
  };
  mkdirSync(join(root, "docs", "evidence"), { recursive: true });
  writeFileSync(
    join(root, "docs", "evidence", "batch2-demo-latest.json"),
    `${JSON.stringify(report, null, 2)}\n`,
  );
  console.log(
    ok
      ? "batch2-integration-demo: all stages passed"
      : "batch2-integration-demo: FAILED (see docs/evidence/batch2-demo-latest.json)",
  );
  process.exit(ok ? 0 : 1);
}

// Stage 1: headless Rust flow across every Batch 2 system.
const demo = sh(
  "cargo",
  ["run", "--quiet", "-p", "batch2_demo"],
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
const failedStep = (flow.steps ?? []).find((s) => !s.ok);
stage(
  "rust.flow",
  failedStep === undefined,
  failedStep === undefined
    ? `${flow.steps.length} steps ok (task ${flow.metrics.taskStatus}, ${flow.metrics.receipts} receipts, ${flow.metrics.originalTokens}->${flow.metrics.compactedTokens} tokens)`
    : `${failedStep.name}: ${failedStep.detail}`,
);

// Stage 2: Void Neon topology reflects the flow (mapping covered by tests).
const neon = sh("pnpm", [
  "--filter",
  "ui",
  "exec",
  "vitest",
  "run",
  "src/features/void-neon/",
]);
stage(
  "void-neon.topology",
  neon.ok,
  neon.ok ? "topology suites green" : neon.stderr.slice(-1500),
);

// Stage 3: audit swarm validates the result (also refreshes docs/reports).
const swarm = sh("pnpm", ["audit:swarm"]);
const swarmOk = swarm.ok && /critical 0/.test(swarm.stdout);
stage(
  "audit.swarm",
  swarmOk,
  swarmOk
    ? (swarm.stdout.match(/Talos audit: .*/) ?? ["swarm green"])[0]
    : swarm.stderr.slice(-1500),
);

finish(true);
