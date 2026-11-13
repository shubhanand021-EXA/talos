#!/usr/bin/env node
/**
 * Talos parity matrix validator + charter checks (Prompt 009).
 * Zero dependencies (repo rule for scripts/): node builtins only.
 *
 * Usage:
 *   node scripts/parity/validate-matrix.mjs --check   # verify matrix + SUMMARY.md + charter (CI gate; exit 1 on any failure)
 *   node scripts/parity/validate-matrix.mjs --write   # regenerate docs/parity/SUMMARY.md, then verify everything
 *
 * Rules enforced (drift or violation => FAIL, exit 1):
 *  1. Schema: version string, rows array; each row has id/category/capability/
 *     priority/competitors/talos_current/evidence (+ optional note).
 *  2. Categories: exactly the 14 locked categories. Priorities: P0|P1|P2.
 *     Statuses: native|partial|absent|unknown.
 *  3. Competitors: exactly the 8 sanctioned keys; charter rule below pins values.
 *  4. Unique row ids.
 *  5. native/partial rows cite existing evidence (file/dir must exist;
 *     "path::symbol" must also contain the symbol). partial rows need a note.
 *  6. SUMMARY.md must equal the deterministic regeneration (never hand-edit).
 *  7. Charter: LICENSE is Apache-2.0 across manifests; zero talos.dev in live
 *     policy surfaces; reports/001-008 present; competitor cells uniformly
 *     "unknown" (Prompt 013 owns the sanctioned import); docs/evidence/FLAKES.md
 *     exists with no overdue OPEN rows.
 */

import {
  existsSync,
  readdirSync,
  readFileSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "..", "..");
const MATRIX_PATH = join(root, "docs", "parity", "CAPABILITY_MATRIX.json");
const SUMMARY_PATH = join(root, "docs", "parity", "SUMMARY.md");

const CATEGORIES = [
  "coding",
  "terminal",
  "browser",
  "computer-use",
  "orchestration",
  "memory",
  "skills",
  "scheduling",
  "channels",
  "artifacts",
  "voice",
  "enterprise",
  "extensibility",
  "platform",
];

const COMPETITORS = [
  "openclaw",
  "hermes",
  "open_interpreter",
  "opencode",
  "claude_code",
  "codex",
  "abacus",
  "deepseek_harness",
];

const PRIORITIES = ["P0", "P1", "P2"];
const STATUSES = ["native", "partial", "absent", "unknown"];

// Prompts whose batch reports must exist (extend when new prompts merge).
const REQUIRED_REPORTS = [
  "001",
  "002",
  "003",
  "004",
  "005",
  "006",
  "007",
  "008",
];

// Live policy surfaces: talos.dev must not appear here (the updater endpoint
// in code/config/docs is a designed, not-yet-public channel — out of scope).
const POLICY_SURFACES = [
  "README.md",
  "SECURITY.md",
  "SUPPORT.md",
  "CODE_OF_CONDUCT.md",
  join("docs", "SECURITY.md"),
  join(".github", "ISSUE_TEMPLATE", "config.yml"),
  join(".github", "ISSUE_TEMPLATE", "security_report.yml"),
  join(".github", "ISSUE_TEMPLATE", "bug_report.yml"),
  join(".github", "ISSUE_TEMPLATE", "feature_request.yml"),
  join(".github", "ISSUE_TEMPLATE", "question.yml"),
];

const failures = [];
const fail = (check, message) => failures.push({ check, message });

function loadMatrix() {
  let raw;
  try {
    raw = readFileSync(MATRIX_PATH, "utf8");
  } catch {
    fail("matrix-exists", `missing ${MATRIX_PATH}`);
    return null;
  }
  try {
    return JSON.parse(raw);
  } catch (err) {
    fail("matrix-parses", `invalid JSON: ${err.message}`);
    return null;
  }
}

function validateSchema(matrix) {
  if (typeof matrix.version !== "string" || matrix.version.length === 0) {
    fail("schema-version", "matrix.version must be a non-empty string");
  }
  if (!Array.isArray(matrix.rows)) {
    fail("schema-rows", "matrix.rows must be an array");
    return;
  }
  const seen = new Set();
  for (const [index, row] of matrix.rows.entries()) {
    const where = `rows[${index}]${row && row.id ? ` (${row.id})` : ""}`;
    if (!row || typeof row !== "object") {
      fail("schema-row", `${where}: row must be an object`);
      continue;
    }
    if (typeof row.id !== "string" || row.id.length === 0) {
      fail("schema-id", `${where}: id must be a non-empty string`);
    } else if (seen.has(row.id)) {
      fail("schema-unique-id", `duplicate row id: ${row.id}`);
    } else {
      seen.add(row.id);
    }
    if (!CATEGORIES.includes(row.category)) {
      fail(
        "schema-category",
        `${where}: category must be one of ${CATEGORIES.join(", ")}`,
      );
    }
    if (typeof row.capability !== "string" || row.capability.trim() === "") {
      fail("schema-capability", `${where}: capability must be non-empty`);
    }
    if (!PRIORITIES.includes(row.priority)) {
      fail(
        "schema-priority",
        `${where}: priority must be one of ${PRIORITIES.join(", ")}`,
      );
    }
    if (!row.competitors || typeof row.competitors !== "object") {
      fail("schema-competitors", `${where}: competitors must be an object`);
    } else {
      const keys = Object.keys(row.competitors).sort();
      const want = [...COMPETITORS].sort();
      if (JSON.stringify(keys) !== JSON.stringify(want)) {
        fail(
          "schema-competitors",
          `${where}: competitors keys must be exactly [${want.join(", ")}]`,
        );
      }
      for (const key of want) {
        if (!STATUSES.includes(row.competitors[key])) {
          fail(
            "schema-competitor-status",
            `${where}: competitors.${key} must be one of ${STATUSES.join(", ")}`,
          );
        }
      }
    }
    if (!STATUSES.includes(row.talos_current)) {
      fail(
        "schema-status",
        `${where}: talos_current must be one of ${STATUSES.join(", ")}`,
      );
    }
    if (!Array.isArray(row.evidence)) {
      fail("schema-evidence", `${where}: evidence must be an array`);
    } else if (
      (row.talos_current === "native" || row.talos_current === "partial") &&
      row.evidence.length === 0
    ) {
      fail(
        "evidence-required",
        `${where}: native/partial rows must cite evidence`,
      );
    }
    if (row.talos_current === "partial") {
      if (typeof row.note !== "string" || row.note.trim() === "") {
        fail(
          "partial-note",
          `${where}: partial rows must document the gap in "note"`,
        );
      }
    }
  }
}

function validateEvidence(matrix) {
  for (const row of matrix.rows) {
    if (!Array.isArray(row.evidence)) continue;
    for (const entry of row.evidence) {
      if (typeof entry !== "string" || entry.trim() === "") {
        fail(
          "evidence-entry",
          `${row.id}: evidence entries must be non-empty strings`,
        );
        continue;
      }
      const [rel, symbol] = entry.split("::");
      const abs = resolve(root, rel);
      let ok = false;
      try {
        const st = statSync(abs);
        ok = st.isFile() || st.isDirectory();
      } catch {
        ok = false;
      }
      const inside = abs === root || abs.startsWith(root + sep);
      if (!ok || !inside) {
        fail("evidence-exists", `${row.id}: evidence path missing: ${rel}`);
        continue;
      }
      // Never cite build output or VCS internals as evidence.
      const lower = rel.replace(/\\/g, "/").toLowerCase();
      if (
        lower.includes("node_modules/") ||
        lower.includes("/.git/") ||
        lower.includes("/target/") ||
        lower.includes("/dist/") ||
        lower.includes(".next/")
      ) {
        fail(
          "evidence-no-build-output",
          `${row.id}: evidence must not be build output: ${rel}`,
        );
      }
      if (symbol !== undefined) {
        let text = "";
        try {
          text = readFileSync(abs, "utf8");
        } catch {
          text = "";
        }
        if (!text.includes(symbol)) {
          fail(
            "evidence-symbol",
            `${row.id}: symbol not found in ${rel}: ${symbol}`,
          );
        }
      }
    }
  }
}

function summarize(matrix) {
  const byStatus = { native: 0, partial: 0, absent: 0, unknown: 0 };
  const byCategory = {};
  for (const category of CATEGORIES) {
    byCategory[category] = {
      native: 0,
      partial: 0,
      absent: 0,
      unknown: 0,
      total: 0,
    };
  }
  let p0Total = 0;
  let p0Native = 0;
  for (const row of matrix.rows) {
    byStatus[row.talos_current] += 1;
    const cell = byCategory[row.category];
    if (cell) {
      cell[row.talos_current] += 1;
      cell.total += 1;
    }
    if (row.priority === "P0") {
      p0Total += 1;
      if (row.talos_current === "native") p0Native += 1;
    }
  }
  return { byStatus, byCategory, p0Total, p0Native };
}

function renderSummary(matrix, summary) {
  const total = matrix.rows.length;
  const lines = [
    "# Talos Parity Summary (generated — do not hand-edit)",
    "",
    "> Generated by `node scripts/parity/validate-matrix.mjs --write` from",
    "> `docs/parity/CAPABILITY_MATRIX.json`. The validator (`--check`) fails",
    "> CI when this file drifts from a deterministic regeneration.",
    "",
    `Matrix version: \`${matrix.version}\` · rows: ${total}`,
    "",
    "## Overall",
    "",
    "| Status | Rows |",
    "| ------ | ---- |",
    `| native | ${summary.byStatus.native} |`,
    `| partial | ${summary.byStatus.partial} |`,
    `| absent | ${summary.byStatus.absent} |`,
    `| unknown | ${summary.byStatus.unknown} |`,
    "",
    `P0 coverage: **${summary.p0Native}/${summary.p0Total} native** (${summary.p0Total === 0 ? "n/a" : `${Math.round((summary.p0Native / summary.p0Total) * 100)}%`}).`,
    "",
    "## Per category",
    "",
    "| Category | Native | Partial | Absent | Unknown | Total |",
    "| -------- | ------ | ------- | ------ | ------- | ----- |",
  ];
  for (const category of CATEGORIES) {
    const cell = summary.byCategory[category];
    lines.push(
      `| ${category} | ${cell.native} | ${cell.partial} | ${cell.absent} | ${cell.unknown} | ${cell.total} |`,
    );
  }
  lines.push(
    "",
    "Competitor columns are uniformly `unknown` until the sanctioned",
    "architect baseline import (Prompt 013). Unknown is honest; guessing",
    "fails the charter check.",
    "",
  );
  return `${lines.join("\n")}`;
}

function validateSummary(matrix) {
  const expected = renderSummary(matrix, summarize(matrix));
  let actual = null;
  try {
    actual = readFileSync(SUMMARY_PATH, "utf8");
  } catch {
    actual = null;
  }
  if (actual !== expected) {
    fail(
      "summary-drift",
      `docs/parity/SUMMARY.md is missing or stale — regenerate with --write (never hand-edit)`,
    );
  }
}

function collectFiles(dir, out) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (entry.name === "node_modules" || entry.name === ".git") continue;
    const full = join(dir, entry.name);
    if (entry.isDirectory()) {
      if (
        entry.name === "target" ||
        entry.name === "dist" ||
        entry.name === ".next"
      ) {
        continue;
      }
      collectFiles(full, out);
    } else if (entry.isFile()) {
      out.push(full);
    }
  }
}

function validateLicenses() {
  const licensePath = join(root, "LICENSE");
  let text = "";
  try {
    text = readFileSync(licensePath, "utf8");
  } catch {
    fail("license-file", "LICENSE file missing");
    return;
  }
  if (!text.trimStart().startsWith("Apache License")) {
    fail("license-text", "LICENSE must be the Apache-2.0 text");
  }
  const files = [];
  collectFiles(root, files);
  const cargoFiles = files.filter((f) => f.endsWith("Cargo.toml"));
  let workspaceRootOk = false;
  for (const file of cargoFiles) {
    const content = readFileSync(file, "utf8");
    for (const line of content.split("\n")) {
      const match = line.match(/^\s*license\s*=\s*"([^"]+)"/);
      if (match && match[1] !== "Apache-2.0") {
        fail(
          "license-manifest",
          `${file}: license = "${match[1]}" (must be Apache-2.0)`,
        );
      }
    }
    if (
      file === join(root, "crates", "Cargo.toml") &&
      content.includes('license = "Apache-2.0"')
    ) {
      workspaceRootOk = true;
    }
  }
  if (!workspaceRootOk) {
    fail(
      "license-workspace",
      "crates/Cargo.toml must declare license = Apache-2.0",
    );
  }
  const pkgFiles = files.filter((f) => f.endsWith("package.json"));
  for (const file of pkgFiles) {
    let json;
    try {
      json = JSON.parse(readFileSync(file, "utf8"));
    } catch {
      continue;
    }
    if (typeof json.license === "string" && json.license !== "Apache-2.0") {
      fail(
        "license-manifest",
        `${file}: license = "${json.license}" (must be Apache-2.0)`,
      );
    }
  }
}

function validateNoTalosDev() {
  // Blocker class from Prompt 004: the Sidero-owned security@ address must be
  // gone. updates.talos.dev is the reserved (not-yet-live) update channel,
  // explicitly documented as a placeholder — allowed.
  for (const rel of POLICY_SURFACES) {
    const abs = join(root, rel);
    if (!existsSync(abs)) continue;
    const text = readFileSync(abs, "utf8");
    if (/security@talos\.dev/i.test(text)) {
      fail(
        "talos-dev",
        `${rel} references security@talos.dev (use private reporting)`,
      );
    }
    const stripped = text.replace(/updates\.talos\.dev/gi, "");
    if (/talos\.dev/i.test(stripped)) {
      fail("talos-dev", `${rel} mentions talos.dev (use private reporting)`);
    }
  }
}

function validateReports() {
  const entries = new Set(readdirSync(join(root, "reports")));
  for (const nnn of REQUIRED_REPORTS) {
    const found = [...entries].some((name) => name.startsWith(`${nnn}-`));
    if (!found) {
      fail(
        "reports-present",
        `reports/${nnn}-*.md missing (merged prompt without a report)`,
      );
    }
  }
}

function validateCompetitorsUnknown(matrix) {
  for (const row of matrix.rows) {
    for (const key of COMPETITORS) {
      if (row.competitors && row.competitors[key] !== "unknown") {
        fail(
          "competitors-unknown",
          `${row.id}: competitors.${key} is "${row.competitors[key]}" (only the Prompt-013 sanctioned import may fill competitor cells)`,
        );
      }
    }
  }
}

function validateFlakes() {
  const flakesPath = join(root, "docs", "evidence", "FLAKES.md");
  let text = "";
  try {
    text = readFileSync(flakesPath, "utf8");
  } catch {
    fail("flakes-exists", "docs/evidence/FLAKES.md missing");
    return;
  }
  const today = new Date().toISOString().slice(0, 10);
  for (const line of text.split("\n")) {
    if (!line.startsWith("| `")) continue;
    const cells = line.split("|").map((c) => c.trim());
    // | test | first seen | owner | deadline | status |
    const deadline = cells[4] || "";
    const status = cells[5] || "";
    if (!/^\d{4}-\d{2}-\d{2}$/.test(deadline)) continue;
    if (/^open/i.test(status) && deadline < today) {
      fail(
        "flakes-overdue",
        `overdue OPEN quarantine row (deadline ${deadline} < ${today}): ${cells[1].slice(0, 80)}`,
      );
    }
  }
}

function main() {
  const args = new Set(process.argv.slice(2));
  const matrix = loadMatrix();
  if (matrix) {
    validateSchema(matrix);
    // Row-dependent checks run whenever the file parses to a rows array —
    // never gated on other failures, so one violation cannot mask another.
    if (matrix && Array.isArray(matrix.rows)) {
      validateEvidence(matrix);
      if (args.has("--write")) {
        writeFileSync(SUMMARY_PATH, renderSummary(matrix, summarize(matrix)));
      }
      validateSummary(matrix);
      validateCompetitorsUnknown(matrix);
    }
  }
  validateLicenses();
  validateNoTalosDev();
  validateReports();
  validateFlakes();
  if (failures.length > 0) {
    console.error(`parity validation FAILED (${failures.length} finding(s)):`);
    for (const finding of failures) {
      console.error(`- [${finding.check}] ${finding.message}`);
    }
    process.exit(1);
  }
  const rows = matrix ? matrix.rows.length : 0;
  console.log(`parity validation ok: ${rows} rows, charter checks green`);
}

main();
