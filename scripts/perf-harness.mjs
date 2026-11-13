#!/usr/bin/env node
/**
 * Talos P3 performance harness (Prompt 012, Phase A). Deterministic, headless,
 * mock-data, fixed seeds; node builtins only (repo rule for scripts/).
 *
 * Usage:
 *   node scripts/perf-harness.mjs --runs 20 [--check] [--out <path>]
 *
 * Measures CPU-side proxies (fidelity: mock — never presented as verified
 * device numbers) plus real dist bytes when apps/ui/dist exists:
 *   perf.spawnP95Ms        process-spawn floor (cold-start proxy floor)
 *   perf.eventCommitP95Ms   1000-event feed commit (interaction proxy)
 *   perf.frameP95Ms         120-frame scene-update sim (frame-budget proxy)
 *   perf.topologyBuildP95Ms algorithmic mirror of buildLiveTopology
 *   perf.idleRssMB          node RSS after the workload (RAM proxy)
 * --check compares p95 values against .talos/baselines.json failAt gates
 * (exit 1 on breach) and prints WARN on warnAt breaches. Every number is
 * tagged with os/arch/node so cross-machine comparison stays honest.
 */

import { execFileSync } from "node:child_process";
import {
  existsSync,
  readFileSync,
  readdirSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { gzipSync } from "node:zlib";
import { dirname, join, resolve } from "node:path";
import { cpus, platform, arch } from "node:os";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "..");

const SEED = 0x51ab3d;
const FRAMES = 120;
const NODES = 120;
const EVENTS = 1000;

/** Deterministic LCG (same family as the viewport Backdrop). */
function lcg(seed) {
  let state = seed >>> 0;
  return () => {
    state = (state * 1664525 + 1013904223) >>> 0;
    return state / 0xffffffff;
  };
}

function percentile(sorted, p) {
  return sorted[
    Math.min(sorted.length - 1, Math.floor((p / 100) * sorted.length))
  ];
}

function summarize(samples) {
  const sorted = [...samples].sort((a, b) => a - b);
  return {
    p50: percentile(sorted, 50),
    p95: percentile(sorted, 95),
    n: sorted.length,
  };
}

function dirBytes(dir) {
  let total = 0;
  let entries = [];
  try {
    entries = readdirSync(dir);
  } catch {
    return null;
  }
  for (const entry of entries) {
    const full = join(dir, entry);
    try {
      const stat = statSync(full);
      total += stat.isDirectory() ? (dirBytes(full) ?? 0) : stat.size;
    } catch {
      continue;
    }
  }
  return total;
}

/** Fixed synthetic feed (mirrors countFeedSignals shapes). */
function makeEvents() {
  const next = lcg(SEED);
  return Array.from({ length: EVENTS }, (_, i) => ({
    id: `event-${i}`,
    actor: next() < 0.5 ? "browser" : "computer",
    taskId: `task-${Math.floor(next() * 48)}`,
  }));
}

/** One event-commit: count browser/computer signals (production shape). */
function commitEvents(events) {
  let browser = 0;
  let computer = 0;
  for (const event of events) {
    if (event.actor === "browser") browser += 1;
    else computer += 1;
  }
  return browser + computer;
}

/** Fixed 120-node graph (golden-spiral layout, production shape). */
function makeGraph() {
  const next = lcg(SEED);
  const golden = Math.PI * (3 - Math.sqrt(5));
  const nodes = Array.from({ length: NODES }, (_, i) => ({
    x: Math.cos(i * golden) * (3.2 + i * 0.05),
    y: (next() - 0.5) * 2,
    z: Math.sin(i * golden) * (3.2 + i * 0.05),
    pulse: next(),
  }));
  return { nodes, edges: NODES * 2 };
}

/** One scene-update frame: pulse lerp over lanes (LinkFlow shape). */
function updateFrame(graph, t) {
  let acc = 0;
  for (let i = 0; i < graph.nodes.length; i += 1) {
    const node = graph.nodes[i];
    const p = (t + i * 0.13) % 1;
    acc += node.x * p + node.y * (1 - p) + node.z * p + node.pulse;
  }
  return acc;
}

/** Algorithmic mirror of buildLiveTopology (24 agents + 48 tasks). */
function buildTopologyMirror() {
  const next = lcg(SEED);
  const golden = Math.PI * (3 - Math.sqrt(5));
  const nodes = [{ id: "talos-core" }];
  const edges = [];
  for (let i = 0; i < 24; i += 1) {
    const angle = i * golden;
    nodes.push({
      id: `agent-${i}`,
      x: Math.cos(angle) * 3.2,
      z: Math.sin(angle) * 3.2,
    });
    edges.push({ source: "talos-core", target: `agent-${i}` });
  }
  for (let i = 0; i < 48; i += 1) {
    nodes.push({ id: `task-${i}`, x: next() * 8 - 4, z: next() * 8 - 4 });
    edges.push({ source: `agent-${i % 24}`, target: `task-${i}` });
  }
  return nodes.length + edges.length;
}

function parseArgs(argv) {
  const args = {
    runs: 20,
    check: false,
    out: join(root, "docs", "evidence", "perf-harness-latest.json"),
  };
  for (let i = 0; i < argv.length; i += 1) {
    if (argv[i] === "--runs")
      args.runs = Math.max(1, Math.min(100, Number(argv[i + 1]) || 20));
    if (argv[i] === "--check") args.check = true;
    if (argv[i] === "--out") args.out = resolve(argv[i + 1] ?? args.out);
  }
  return args;
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  const env = {
    os: platform(),
    arch: arch(),
    node: process.version,
    cpus: cpus().length,
    seed: SEED,
    runs: args.runs,
  };

  const spawnSamples = [];
  const commitSamples = [];
  const frameSamples = [];
  const topoSamples = [];
  for (let r = 0; r < args.runs; r += 1) {
    const spawnStart = performance.now();
    execFileSync(process.execPath, ["-e", "process.exit(0)"], {
      stdio: "ignore",
    });
    spawnSamples.push(performance.now() - spawnStart);

    const events = makeEvents();
    const commitStart = performance.now();
    commitEvents(events);
    commitSamples.push(performance.now() - commitStart);

    const graph = makeGraph();
    const frameStart = performance.now();
    for (let f = 0; f < FRAMES; f += 1) updateFrame(graph, f / FRAMES);
    frameSamples.push((performance.now() - frameStart) / FRAMES);

    const topoStart = performance.now();
    buildTopologyMirror();
    topoSamples.push(performance.now() - topoStart);
  }

  const rssMB = process.memoryUsage().rss / (1024 * 1024);
  const dist = dirBytes(join(root, "apps", "ui", "dist"));
  const metrics = {
    "perf.spawnP95Ms": summarize(spawnSamples).p95,
    "perf.eventCommitP95Ms": summarize(commitSamples).p95,
    "perf.frameP95Ms": summarize(frameSamples).p95,
    "perf.topologyBuildP95Ms": summarize(topoSamples).p95,
    "perf.idleRssMB": rssMB,
  };
  const detail = {
    spawn: summarize(spawnSamples),
    eventCommit: summarize(commitSamples),
    frame: summarize(frameSamples),
    topologyBuild: summarize(topoSamples),
  };

  const report = {
    generatedAt: new Date().toISOString(),
    env,
    fidelity: "mock",
    fidelityNote:
      "CPU-side deterministic proxies (fixed seeds, mock data). Not device/browser/GPU measurements; see docs/PERFORMANCE.md for the honest P3 mapping.",
    metrics: Object.fromEntries(
      Object.entries(metrics).map(([k, v]) => [k, Math.round(v * 1000) / 1000]),
    ),
    detail,
    distBytes: dist,
  };
  writeFileSync(args.out, `${JSON.stringify(report, null, 2)}\n`);

  console.log("| metric | p50 | p95 | unit | env |");
  console.log("| --- | --- | --- | --- | --- |");
  const tag = `${env.os}/${env.arch}/node-${env.node}`;
  for (const [name, s] of Object.entries(detail)) {
    console.log(
      `| ${name} | ${s.p50.toFixed(3)} | ${s.p95.toFixed(3)} | ms | ${tag} |`,
    );
  }
  console.log(`| idleRss | — | ${rssMB.toFixed(1)} | MB | ${tag} |`);
  console.log(
    `| dist | — | ${dist ?? "missing (run pnpm build)"} | bytes | ${tag} |`,
  );

  if (args.check) {
    const raw = readFileSync(join(root, ".talos", "baselines.json"), "utf8");
    const baselines = JSON.parse(raw).metrics;
    let failed = false;
    for (const [name, value] of Object.entries(metrics)) {
      const gate = baselines[name];
      if (gate === undefined || gate.referenceOnly === true) continue;
      if (gate.failAt !== undefined && value > gate.failAt) {
        console.error(`FAIL ${name} = ${value} exceeds failAt ${gate.failAt}`);
        failed = true;
      } else if (gate.warnAt !== undefined && value > gate.warnAt) {
        console.log(`WARN ${name} = ${value} exceeds warnAt ${gate.warnAt}`);
      } else {
        console.log(`ok ${name} = ${value} within budget`);
      }
    }
    if (!existsSync(join(root, "apps", "ui", "dist", "index.html"))) {
      console.log(
        "WARN dist missing — run pnpm build first (not a product failure)",
      );
    }
    if (failed) process.exit(1);
  }
}

main();
