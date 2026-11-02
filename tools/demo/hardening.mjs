// Proof of Hardening: end-to-end Batch 4 integration demo.
// Runs headlessly: file-backed durable state (WAL) + encrypted store +
// crash recovery -> validated DAG planning (keyword + stub-LLM) ->
// supervisor limits -> child budgets -> real verified specialists under the
// OS sandbox -> kill/reap + fail-closed checks -> GEPA skill learning ->
// backup/restore -> fidelity truthfulness -> chain validity. Then verifies
// the same database through the real headless CLI (`talos task list`,
// `talos evidence verify`, `talos skill list`).
// Exits non-zero on the first failing stage. Full-mock by default (CI-safe,
// deterministic, no network/display/keys; the OS sandbox runs live wherever
// a backend exists, fail-closed otherwise).
// Flags: --sandbox-live requires a live backend by platform name.
// Report: docs/evidence/batch4-demo-report.json (steps, CLI checks, metrics).

import { execFileSync } from "node:child_process";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const shell = process.platform === "win32";

let requireLiveSandbox = false;
for (const arg of process.argv.slice(2)) {
  if (arg === "--sandbox-live") {
    requireLiveSandbox = true;
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
      timeout: 900_000,
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

// Preflight: UI contracts dist (same gate as the awakening demo).
if (!existsSync(join(root, "packages", "contracts", "dist", "index.js"))) {
  stage(
    "preflight.contracts-dist",
    false,
    "packages/contracts/dist/index.js missing — run: pnpm --filter @talos/contracts build",
  );
}
stage("preflight.contracts-dist", true, "contracts dist present");

// Shared demo dir: the Rust demo writes the file-backed DB here, and the
// CLI checks below read the same ledger (TALOS_DATA_DIR points at main/).
const demoDir = mkdtempSync(join(tmpdir(), "talos-hardening-"));
const demoEnv = { TALOS_HARDENING_DIR: demoDir };
if (requireLiveSandbox) {
  demoEnv.TALOS_HARDENING_SANDBOX = "require-live";
}

function finish(ok) {
  const report = {
    scenario: "proof-of-hardening",
    mode: requireLiveSandbox ? "sandbox-live" : "full-mock",
    ok,
    generatedAt: new Date().toISOString(),
    stages,
    flow: finish.flow ?? null,
    cli: finish.cli ?? null,
    commit: process.env.TALOS_DEMO_COMMIT ?? null,
  };
  mkdirSync(join(root, "docs", "evidence"), { recursive: true });
  writeFileSync(
    join(root, "docs", "evidence", "batch4-demo-report.json"),
    `${JSON.stringify(report, null, 2)}\n`,
  );
  try {
    rmSync(demoDir, { recursive: true, force: true });
  } catch {
    // Best effort: CI temp is reclaimed regardless.
  }
  console.log(
    ok
      ? "proof-of-hardening: all stages passed"
      : "proof-of-hardening: FAILED (see docs/evidence/batch4-demo-report.json)",
  );
  process.exit(ok ? 0 : 1);
}

// Stage 1: headless Rust flow across every Batch 4 hardening system.
const demo = sh(
  "cargo",
  ["run", "--quiet", "-p", "hardening_demo"],
  join(root, "crates"),
  demoEnv,
);
let flow = null;
try {
  flow = JSON.parse(demo.stdout.slice(demo.stdout.indexOf("{")));
} catch {
  flow = null;
}
if (!demo.ok || flow === null || flow.ok !== true) {
  stage(
    "rust.flow",
    false,
    (demo.stderr || demo.stdout || "no output").slice(-2000),
  );
}
finish.flow = flow;
const failedStep = (flow.steps ?? []).find((s) => !s.ok);
stage(
  "rust.flow",
  failedStep === undefined,
  failedStep === undefined
    ? `${flow.steps.length} steps ok in ${flow.metrics.totalMs}ms (mode ${flow.mode}, backend ${flow.metrics.sandboxBackend}, chain ${flow.metrics.chainChecked} links, completion ${flow.metrics.completionRate})`
    : `${failedStep.name}: see report`,
);

// Stage 2: the real headless CLI verifies the same ledger.
const cliEnv = { TALOS_DATA_DIR: join(demoDir, "main") };
function talosJson(args) {
  const run = sh(
    "cargo",
    ["run", "--quiet", "-p", "talos_cli", "--", ...args],
    join(root, "crates"),
    cliEnv,
  );
  if (!run.ok) {
    return { ok: false, detail: (run.stderr || run.stdout).slice(-1200) };
  }
  const lines = run.stdout
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.startsWith("{"));
  const events = [];
  for (const line of lines) {
    try {
      events.push(JSON.parse(line));
    } catch {
      return { ok: false, detail: `unparsable JSONL: ${line.slice(0, 200)}` };
    }
  }
  return { ok: true, events };
}

{
  const listed = talosJson([
    "--format",
    "json",
    "task",
    "list",
    "--status",
    "completed",
  ]);
  const parent = (listed.events ?? [])
    .flatMap((e) => e.data ?? [])
    .find((t) => t?.id === "hardening-parent");
  stage(
    "cli.task_list",
    listed.ok &&
      parent?.status === "completed" &&
      parent?.fidelity === "verified",
    listed.ok
      ? `hardening-parent: status=${parent?.status} fidelity=${parent?.fidelity}`
      : listed.detail,
  );
}

{
  const verified = talosJson(["--format", "json", "evidence", "verify"]);
  const report = verified.events?.find((e) => e.type === "evidence_verified");
  stage(
    "cli.evidence_verify",
    verified.ok &&
      report?.event_chain_valid === true &&
      report?.evidence_chain_valid === true,
    verified.ok ? (report?.detail ?? "chain valid") : verified.detail,
  );
}

{
  const skills = talosJson(["--format", "json", "skill", "list"]);
  const names = (skills.events ?? [])
    .flatMap((e) => e.data ?? [])
    .map((s) => s?.id);
  const expected = flow.metrics?.skillId;
  stage(
    "cli.skill_list",
    skills.ok && names.includes(expected),
    skills.ok ? `skills=[${names.join(", ")}]` : skills.detail,
  );
}
finish.cli = { checked: ["task list", "evidence verify", "skill list"] };

finish(true);
