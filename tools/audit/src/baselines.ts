import { join } from "node:path";
import type { AuditReport, Finding } from "./types.ts";
import { fileExists, readJson } from "./util.ts";

/** One baseline entry: reference value plus regression gates. */
export interface BaselineEntry {
  value: number;
  unit: string;
  warnAt?: number;
  failAt?: number;
  referenceOnly?: boolean;
  description: string;
}

/** Baselines file shape (`.talos/baselines.json`). */
export interface Baselines {
  version: number;
  metrics: Record<string, BaselineEntry>;
}

/** Metric names the swarm collects (single source for runner + agents). */
export const KNOWN_METRICS = [
  "ui.distBytes",
  "ui.mainJsGzipBytes",
  "ui.voidNeonChunkGzipBytes",
  "rust.evidenceSubsetMs",
  "rust.agentRuntimeSubsetMs",
  "startup.toolchainProbeMs",
  "swarm.totalDurationMs",
  "telemetry.suiteMs",
  "telemetry.instrumentationSites",
  "telemetry.costDriftUsd",
] as const;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isEntry(value: unknown): value is BaselineEntry {
  return (
    isRecord(value) &&
    typeof value["value"] === "number" &&
    typeof value["unit"] === "string" &&
    typeof value["description"] === "string" &&
    (value["warnAt"] === undefined || typeof value["warnAt"] === "number") &&
    (value["failAt"] === undefined || typeof value["failAt"] === "number")
  );
}

/** Load `.talos/baselines.json`. Returns null when absent/invalid (never throws). */
export function loadBaselines(root: string): Baselines | null {
  const path = join(root, ".talos", "baselines.json");
  if (!fileExists(path)) {
    return null;
  }
  try {
    const raw: unknown = readJson(path);
    if (!isRecord(raw) || typeof raw["version"] !== "number") {
      return null;
    }
    if (!isRecord(raw["metrics"])) {
      return null;
    }
    const metrics: Record<string, BaselineEntry> = {};
    for (const [name, entry] of Object.entries(raw["metrics"])) {
      if (isEntry(entry)) {
        metrics[name] = entry;
      }
    }
    return { version: raw["version"] as number, metrics };
  } catch {
    return null;
  }
}

/** Compare one measured metric against its baseline entry. */
export function compareMetric(
  name: string,
  value: number,
  baselines: Baselines,
): Finding | null {
  const entry = baselines.metrics[name];
  if (entry === undefined || entry.referenceOnly === true) {
    return null;
  }
  if (entry.failAt !== undefined && value > entry.failAt) {
    return {
      severity: "critical",
      check: "baseline-regression",
      message: `${name} = ${value} ${entry.unit} exceeds fail gate ${entry.failAt} (baseline ${entry.value})`,
      evidence: `.talos/baselines.json:${name}`,
    };
  }
  if (entry.warnAt !== undefined && value > entry.warnAt) {
    return {
      severity: "warning",
      check: "baseline-regression",
      message: `${name} = ${value} ${entry.unit} exceeds warn gate ${entry.warnAt} (baseline ${entry.value})`,
      evidence: `.talos/baselines.json:${name}`,
    };
  }
  return {
    severity: "info",
    check: "baseline-ok",
    message: `${name} = ${value} ${entry.unit} within budget (baseline ${entry.value})`,
    evidence: `.talos/baselines.json:${name}`,
  };
}

/**
 * Compare every collected run metric against baselines.
 * Unknown metrics and reference-only entries produce no findings.
 */
export function evaluateBaselines(
  metrics: Record<string, number>,
  baselines: Baselines | null,
): Finding[] {
  if (baselines === null) {
    return [
      {
        severity: "warning",
        check: "baselines-present",
        message:
          ".talos/baselines.json missing or invalid — regression gates unverifiable",
        evidence: ".talos/baselines.json",
      },
    ];
  }
  const findings: Finding[] = [
    {
      severity: "info",
      check: "baselines-present",
      message: `Baselines v${baselines.version}: ${Object.keys(baselines.metrics).length} metrics with gates`,
      evidence: ".talos/baselines.json",
    },
  ];
  for (const name of Object.keys(metrics).sort()) {
    const finding = compareMetric(name, metrics[name] as number, baselines);
    if (finding !== null) {
      findings.push(finding);
    }
  }
  return findings;
}

/** Shape of the baselines section embedded in Markdown + JSON reports. */
export interface BaselineSummary {
  name: string;
  value: number | null;
  baseline: number | null;
  unit: string;
  status: "ok" | "warn" | "fail" | "reference" | "missing";
}

/** Build a per-metric summary table for reports and the UI dashboard. */
export function summarizeBaselines(
  metrics: Record<string, number>,
  baselines: Baselines | null,
): BaselineSummary[] {
  const names = new Set([
    ...Object.keys(metrics),
    ...Object.keys(baselines?.metrics ?? {}),
  ]);
  return [...names].sort().map((name) => {
    const value = metrics[name];
    const entry = baselines?.metrics[name];
    if (entry === undefined) {
      return {
        name,
        value: value ?? null,
        baseline: null,
        unit: "",
        status: "missing" as const,
      };
    }
    if (value === undefined) {
      return {
        name,
        value: null,
        baseline: entry.value,
        unit: entry.unit,
        status:
          entry.referenceOnly === true
            ? ("reference" as const)
            : ("missing" as const),
      };
    }
    if (entry.referenceOnly === true) {
      return {
        name,
        value,
        baseline: entry.value,
        unit: entry.unit,
        status: "reference" as const,
      };
    }
    if (entry.failAt !== undefined && value > entry.failAt) {
      return {
        name,
        value,
        baseline: entry.value,
        unit: entry.unit,
        status: "fail" as const,
      };
    }
    if (entry.warnAt !== undefined && value > entry.warnAt) {
      return {
        name,
        value,
        baseline: entry.value,
        unit: entry.unit,
        status: "warn" as const,
      };
    }
    return {
      name,
      value,
      baseline: entry.value,
      unit: entry.unit,
      status: "ok" as const,
    };
  });
}

/** Narrow helper for the dashboard: report metrics by another name. */
export function reportMetrics(report: AuditReport): Record<string, number> {
  return report.metrics;
}
