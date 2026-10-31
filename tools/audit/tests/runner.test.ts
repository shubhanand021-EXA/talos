import assert from "node:assert/strict";
import { mkdtempSync, readdirSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, it } from "node:test";
import { exitCodeFor, formatMarkdown, writeReports } from "../src/runner.ts";
import type { AuditReport } from "../src/types.ts";

function fakeReport(status: AuditReport["status"]): AuditReport {
  return {
    generatedAt: "2026-10-31T12:00:00.000Z",
    root: "/repo",
    configVersion: 1,
    status,
    metrics: { "ui.distBytes": 100 },
    summary: {
      agents: 1,
      passed: status === "PASS" ? 1 : 0,
      warned: status === "WARN" ? 1 : 0,
      failed: status === "FAIL" ? 1 : 0,
      findings: status === "PASS" ? 0 : 1,
      critical: status === "FAIL" ? 1 : 0,
      warnings: status === "WARN" ? 1 : 0,
      infos: 0,
    },
    agents: [
      {
        agent: "TestAgent",
        status:
          status === "PASS" ? "pass" : status === "WARN" ? "warn" : "fail",
        findings:
          status === "PASS"
            ? []
            : [
                {
                  severity: status === "FAIL" ? "critical" : "warning",
                  check: "c",
                  message: "m",
                  evidence: "e",
                },
              ],
        durationMs: 1,
      },
    ],
  };
}

describe("runner severity model", () => {
  it("maps critical to FAIL/exit 1, warnings to WARN/exit 0, clean to PASS/exit 0", () => {
    assert.equal(exitCodeFor("FAIL"), 1);
    assert.equal(exitCodeFor("WARN"), 0);
    assert.equal(exitCodeFor("PASS"), 0);
  });

  it("renders markdown with status, summary, baselines, and per-agent sections", () => {
    const md = formatMarkdown(fakeReport("WARN"));
    assert.match(md, /Status: WARN/);
    assert.match(md, /TestAgent — WARN/);
    assert.match(md, /Failed checks & remediation/);
    assert.match(md, /## Baselines/);
    assert.match(md, /ui\.distBytes/);
  });

  it("writes latest + dated report copies", () => {
    const dir = mkdtempSync(join(tmpdir(), "talos-audit-reports-"));
    try {
      const { md, json } = writeReports(fakeReport("PASS"), dir);
      assert.match(md, /audit-latest\.md$/);
      assert.match(json, /audit-latest\.json$/);
      assert.ok(readFileSync(md, "utf8").includes("Status: PASS"));
      assert.ok(
        (JSON.parse(readFileSync(json, "utf8")) as { status: string })
          .status === "PASS",
      );
      assert.ok(readdirSync(dir).some((f) => f === "audit-2026-10-31.md"));
      assert.ok(readdirSync(dir).some((f) => f === "audit-2026-10-31.json"));
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });
});
