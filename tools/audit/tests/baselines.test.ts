import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, it } from "node:test";
import {
  compareMetric,
  evaluateBaselines,
  loadBaselines,
  summarizeBaselines,
  type Baselines,
} from "../src/baselines.ts";

function baselines(): Baselines {
  return {
    version: 1,
    metrics: {
      "ui.distBytes": {
        value: 1000,
        unit: "bytes",
        warnAt: 2000,
        failAt: 4000,
        description: "test",
      },
      "rust.evidenceSubsetMs": {
        value: 500,
        unit: "ms",
        warnAt: 1000,
        failAt: 2000,
        description: "test",
      },
      "tests.ui": {
        value: 77,
        unit: "count",
        referenceOnly: true,
        description: "reference",
      },
    },
  };
}

describe("baselines", () => {
  it("loads the repo baselines file", () => {
    const loaded = loadBaselines("D:\\Private\\talos");
    assert.ok(loaded !== null);
    assert.equal(loaded.version, 1);
    assert.ok(loaded.metrics["ui.distBytes"] !== undefined);
    assert.ok(loaded.metrics["swarm.totalDurationMs"] !== undefined);
  });

  it("returns null for missing or invalid baselines", () => {
    const dir = mkdtempSync(join(tmpdir(), "talos-audit-baselines-"));
    try {
      assert.equal(loadBaselines(dir), null);
      mkdirSync(join(dir, ".talos"), { recursive: true });
      writeFileSync(
        join(dir, ".talos", "baselines.json"),
        JSON.stringify({ nope: true }),
      );
      assert.equal(loadBaselines(dir), null);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("compares metrics against warn/fail gates", () => {
    const base = baselines();
    const ok = compareMetric("ui.distBytes", 1500, base);
    assert.equal(ok?.severity, "info");
    assert.equal(ok?.check, "baseline-ok");
    const warn = compareMetric("ui.distBytes", 2500, base);
    assert.equal(warn?.severity, "warning");
    assert.equal(warn?.check, "baseline-regression");
    const fail = compareMetric("ui.distBytes", 5000, base);
    assert.equal(fail?.severity, "critical");
    assert.equal(fail?.check, "baseline-regression");
  });

  it("skips reference-only and unknown metrics", () => {
    const base = baselines();
    assert.equal(compareMetric("tests.ui", 99999, base), null);
    assert.equal(compareMetric("nope.metric", 1, base), null);
  });

  it("evaluates a full metric set, warning when baselines are absent", () => {
    const missing = evaluateBaselines({ "ui.distBytes": 1 }, null);
    assert.ok(
      missing.some(
        (f) => f.check === "baselines-present" && f.severity === "warning",
      ),
    );
    const evaluated = evaluateBaselines(
      { "ui.distBytes": 1500, "tests.ui": 77 },
      baselines(),
    );
    assert.ok(
      evaluated.some(
        (f) => f.check === "baselines-present" && f.severity === "info",
      ),
    );
    assert.ok(
      evaluated.some(
        (f) => f.check === "baseline-ok" && f.message.includes("ui.distBytes"),
      ),
    );
    assert.ok(!evaluated.some((f) => f.message.includes("tests.ui")));
  });

  it("summarizes gate status per metric for reports and the dashboard", () => {
    const rows = summarizeBaselines(
      { "ui.distBytes": 1500, "rust.evidenceSubsetMs": 5000 },
      baselines(),
    );
    const byName = new Map(rows.map((r) => [r.name, r]));
    assert.equal(byName.get("ui.distBytes")?.status, "ok");
    assert.equal(byName.get("rust.evidenceSubsetMs")?.status, "fail");
    assert.equal(byName.get("tests.ui")?.status, "reference");
  });
});
