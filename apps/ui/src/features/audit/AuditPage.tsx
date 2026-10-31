import { useEffect, useState } from "react";
import { z } from "zod";
import {
  DataTable,
  EmptyState,
  GlassPanel,
  KeyValueList,
  SectionHeader,
  Spinner,
  StatusPill,
  type PillTone,
} from "../../design-system/index.js";
import committedReport from "../../../../../docs/reports/audit-latest.json";
import baselinesFile from "../../../../../.talos/baselines.json";

/** Minimal shape of the committed/live swarm report (validated at runtime). */
const AgentResultSchema = z
  .object({
    agent: z.string().min(1),
    status: z.enum(["pass", "warn", "fail"]),
    findings: z
      .array(
        z.object({
          severity: z.enum(["info", "warning", "critical"]),
          check: z.string().min(1),
          message: z.string().min(1),
          evidence: z.string(),
        }),
      )
      .default([]),
    durationMs: z.number().nonnegative(),
  })
  .strict();

const ReportSnapshotSchema = z.looseObject({
  generatedAt: z.string().min(1),
  status: z.enum(["PASS", "WARN", "FAIL"]),
  summary: z
    .object({
      agents: z.number().int().nonnegative(),
      passed: z.number().int().nonnegative(),
      warned: z.number().int().nonnegative(),
      failed: z.number().int().nonnegative(),
      findings: z.number().int().nonnegative(),
      critical: z.number().int().nonnegative(),
      warnings: z.number().int().nonnegative(),
      infos: z.number().int().nonnegative(),
    })
    .strict(),
  agents: z.array(AgentResultSchema).max(32),
  metrics: z.record(z.string(), z.number()).default({}),
});

type ReportSnapshot = z.infer<typeof ReportSnapshotSchema>;

const BaselineEntrySchema = z
  .object({
    value: z.number(),
    unit: z.string(),
    warnAt: z.number().optional(),
    failAt: z.number().optional(),
    referenceOnly: z.boolean().optional(),
    description: z.string(),
  })
  .strict();

const BaselinesFileSchema = z.looseObject({
  version: z.number(),
  metrics: z.record(z.string(), BaselineEntrySchema),
});

const STATUS_TONE: Record<string, PillTone> = {
  PASS: "ok",
  WARN: "warn",
  FAIL: "error",
  pass: "ok",
  warn: "warn",
  fail: "error",
  ok: "ok",
  warn_gate: "warn",
  fail_gate: "error",
  reference: "neutral",
  missing: "neutral",
};

function gateOf(
  value: number | undefined,
  entry: z.infer<typeof BaselineEntrySchema>,
): "ok" | "warn_gate" | "fail_gate" | "reference" | "missing" {
  if (value === undefined || entry.referenceOnly === true) {
    return entry.referenceOnly === true ? "reference" : "missing";
  }
  if (entry.failAt !== undefined && value > entry.failAt) {
    return "fail_gate";
  }
  if (entry.warnAt !== undefined && value > entry.warnAt) {
    return "warn_gate";
  }
  return "ok";
}

function asReportSnapshot(raw: unknown): ReportSnapshot | null {
  const parsed = ReportSnapshotSchema.safeParse(raw);
  return parsed.success ? parsed.data : null;
}

function asBaselines(
  raw: unknown,
): Record<string, z.infer<typeof BaselineEntrySchema>> {
  const parsed = BaselinesFileSchema.safeParse(raw);
  return parsed.success ? parsed.data.metrics : {};
}

/** Latest swarm report: live dev-server copy when present, else the committed snapshot. */
export function AuditPage() {
  const [report, setReport] = useState<ReportSnapshot | null>(null);
  const [live, setLive] = useState(false);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      setLoading(true);
      try {
        const response = await fetch("reports/audit-latest.json");
        if (!response.ok) {
          throw new Error(`HTTP ${response.status}`);
        }
        const raw: unknown = await response.json();
        const parsed = asReportSnapshot(raw);
        if (!cancelled) {
          if (parsed !== null) {
            setReport(parsed);
            setLive(true);
          } else {
            setReport(asReportSnapshot(committedReport));
            setLive(false);
          }
        }
      } catch {
        if (!cancelled) {
          setReport(asReportSnapshot(committedReport));
          setLive(false);
        }
      } finally {
        if (!cancelled) {
          setLoading(false);
        }
      }
    };
    void load();
    return () => {
      cancelled = true;
    };
  }, []);

  const baselines = asBaselines(baselinesFile);
  const baselineRows = Object.entries(baselines)
    .sort(([a], [b]) => (a < b ? -1 : 1))
    .map(([name, entry]) => {
      const value = report?.metrics[name];
      return { name, entry, value };
    });

  return (
    <div className="flex flex-col gap-4">
      <SectionHeader
        title="Audit"
        subtitle="9-agent swarm — deterministic baseline report"
        action={
          <StatusPill tone={live ? "ok" : "neutral"}>
            {live ? "live report" : "committed snapshot"}
          </StatusPill>
        }
      />
      {loading ? (
        <GlassPanel label="Latest report">
          <Spinner label="Loading audit report" />
        </GlassPanel>
      ) : report === null ? (
        <GlassPanel label="Latest report">
          <EmptyState
            title="No report available"
            message="Run pnpm audit:swarm to generate docs/reports/audit-latest.json."
          />
        </GlassPanel>
      ) : (
        <>
          <div className="grid grid-cols-1 gap-4 2xl:grid-cols-2">
            <GlassPanel label="Latest report">
              <div className="flex items-center justify-between">
                <h3 className="font-display text-sm font-semibold">
                  Swarm {report.status}
                </h3>
                <StatusPill tone={STATUS_TONE[report.status] ?? "neutral"}>
                  {report.status}
                </StatusPill>
              </div>
              <p className="mt-2 text-sm text-slate-400">
                Generated {report.generatedAt}. Full detail lives in
                docs/reports/audit-latest.json (also uploaded by CI as
                talos-audit-reports).
              </p>
              <div className="mt-3">
                <KeyValueList
                  entries={[
                    { key: "passed", value: String(report.summary.passed) },
                    { key: "warned", value: String(report.summary.warned) },
                    { key: "failed", value: String(report.summary.failed) },
                    {
                      key: "findings",
                      value: `${report.summary.findings} (critical ${report.summary.critical}, warnings ${report.summary.warnings}, info ${report.summary.infos})`,
                    },
                    {
                      key: "report",
                      value: "docs/reports/audit-latest.json",
                    },
                  ]}
                />
              </div>
            </GlassPanel>
            <GlassPanel label="Performance baselines">
              <SectionHeader
                title="Baselines"
                subtitle=".talos/baselines.json gates"
              />
              <DataTable
                caption="Performance baselines"
                rows={baselineRows.map(({ name, entry, value }) => ({
                  id: name,
                  measured:
                    value === undefined ? "—" : `${value} ${entry.unit}`,
                  baseline: `${entry.value} ${entry.unit}`,
                  gate: gateOf(value, entry),
                }))}
                keyOf={(row) => row.id}
                columns={[
                  { header: "Metric", render: (row) => row.id },
                  { header: "Measured", render: (row) => row.measured },
                  { header: "Baseline", render: (row) => row.baseline },
                  {
                    header: "Gate",
                    render: (row) => (
                      <StatusPill tone={STATUS_TONE[row.gate] ?? "neutral"}>
                        {row.gate}
                      </StatusPill>
                    ),
                  },
                ]}
              />
            </GlassPanel>
          </div>
          <GlassPanel label="Swarm agents">
            <DataTable
              caption="Agent status grid"
              rows={report.agents.map((agent) => {
                const critical = agent.findings.filter(
                  (f) => f.severity === "critical",
                ).length;
                const warnings = agent.findings.filter(
                  (f) => f.severity === "warning",
                ).length;
                return {
                  id: agent.agent,
                  status: agent.status,
                  duration: `${agent.durationMs}ms`,
                  detail: `${critical} critical · ${warnings} warnings`,
                };
              })}
              keyOf={(row) => row.id}
              columns={[
                { header: "Agent", render: (row) => row.id },
                {
                  header: "Status",
                  render: (row) => (
                    <StatusPill tone={STATUS_TONE[row.status] ?? "neutral"}>
                      {row.status}
                    </StatusPill>
                  ),
                },
                { header: "Duration", render: (row) => row.duration },
                { header: "Findings", render: (row) => row.detail },
              ]}
            />
          </GlassPanel>
        </>
      )}
    </div>
  );
}
