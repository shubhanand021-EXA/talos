import { useCallback, useEffect, useState } from "react";
import type {
  CostBreakdown,
  CostSlice,
  ModelLatency,
  OperationalMetrics,
  SpanSummary,
  ToolStats,
} from "../../bridge/index.js";
import {
  DataTable,
  EmptyState,
  GlassPanel,
  NeonButton,
  SectionHeader,
  Spinner,
  StatusPill,
  type PillTone,
} from "../../design-system/index.js";
import {
  exportTelemetry,
  getCostBreakdown,
  getOperationalMetrics,
  getTraceSummary,
  isBackendUnavailable,
  isTauri,
  setSessionBudget,
} from "../../bridge/index.js";

function pct(value: number): string {
  return `${(value * 100).toFixed(1)}%`;
}

function usd(value: number): string {
  return `$${value.toFixed(6)}`;
}

function CostBars({ rows, unit }: { rows: CostSlice[]; unit: string }) {
  const top = rows.slice(0, 8);
  const max = top.reduce((m, r) => Math.max(m, r.costUsd), 0);
  if (top.length === 0) {
    return <p className="text-xs text-slate-500">No cost recorded yet.</p>;
  }
  return (
    <div className="flex flex-col gap-2">
      {top.map((row) => (
        <div key={row.key} className="flex flex-col gap-1">
          <div className="flex items-center justify-between gap-2 text-xs">
            <span className="truncate font-mono text-slate-300" title={row.key}>
              {row.key}
            </span>
            <span className="shrink-0 text-slate-400">
              {usd(row.costUsd)} · {row.records} {unit}
            </span>
          </div>
          <div
            className="h-1.5 overflow-hidden rounded-full bg-white/5"
            role="img"
            aria-label={`${row.key} ${usd(row.costUsd)}`}
          >
            <div
              className="h-full rounded-full bg-cyan-400/80"
              style={{ width: `${max === 0 ? 0 : (row.costUsd / max) * 100}%` }}
            />
          </div>
        </div>
      ))}
    </div>
  );
}

export function OperationsPage() {
  const [metrics, setMetrics] = useState<OperationalMetrics | null>(null);
  const [costs, setCosts] = useState<CostBreakdown | null>(null);
  const [traces, setTraces] = useState<SpanSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [budgetInput, setBudgetInput] = useState("");
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [m, c, t] = await Promise.all([
        getOperationalMetrics(),
        getCostBreakdown(),
        getTraceSummary(50),
      ]);
      setMetrics(m);
      setCosts(c);
      setTraces(t);
      if (c.sessionBudgetUsd !== null && c.sessionBudgetUsd > 0) {
        setBudgetInput(String(c.sessionBudgetUsd));
      }
    } catch (err) {
      setError(
        err instanceof Error ? err.message : "Failed to load operations data",
      );
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    // Initial load only; Refresh button reuses the same fetcher.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    void refresh();
  }, [refresh]);

  const handleExport = useCallback(async () => {
    setBusy(true);
    setNotice(null);
    try {
      const result = await exportTelemetry();
      setNotice(
        `Exported ${result.spansWritten} spans to ${result.path} (local file, nothing transmitted).`,
      );
    } catch (err) {
      setNotice(
        err instanceof Error
          ? `Export failed: ${err.message}`
          : "Export failed",
      );
    } finally {
      setBusy(false);
    }
  }, []);

  const handleBudget = useCallback(async () => {
    setBusy(true);
    setNotice(null);
    try {
      const trimmed = budgetInput.trim();
      const value = trimmed === "" ? null : Number(trimmed);
      if (value !== null && !(Number.isFinite(value) && value >= 0)) {
        setNotice("Session budget must be a number >= 0 (empty clears it).");
        return;
      }
      await setSessionBudget(value);
      setNotice(
        value === null
          ? "Session budget cleared (metering only)."
          : `Session budget set to $${value} — live actions deny past the cap, warning at 80%.`,
      );
      await refresh();
    } catch (err) {
      setNotice(
        err instanceof Error
          ? `Budget update failed: ${err.message}`
          : "Budget update failed",
      );
    } finally {
      setBusy(false);
    }
  }, [budgetInput, refresh]);

  if (!isTauri()) {
    return (
      <div className="flex flex-col gap-4">
        <SectionHeader
          title="Operations"
          subtitle="Live observability — desktop shell required"
        />
        <GlassPanel label="Backend status">
          <EmptyState
            title="Running in browser preview"
            message="Operational metrics, cost attribution, and traces come from the local Tauri backend. Launch the desktop shell for live data."
          />
        </GlassPanel>
      </div>
    );
  }

  if (loading && metrics === null) {
    return (
      <div className="flex h-64 items-center justify-center">
        <Spinner label="Loading operations data" />
      </div>
    );
  }

  if (error !== null && metrics === null) {
    return (
      <div className="flex flex-col gap-4">
        <SectionHeader title="Operations" subtitle="Live observability" />
        <GlassPanel label="Load error">
          <EmptyState
            title={
              isBackendUnavailable(new Error(error))
                ? "Backend unreachable"
                : "Load failed"
            }
            message={error}
            action={
              <NeonButton
                onClick={() => {
                  void refresh();
                }}
              >
                Retry
              </NeonButton>
            }
          />
        </GlassPanel>
      </div>
    );
  }

  const perModel: ModelLatency[] = metrics?.perModel ?? [];
  const perTool: ToolStats[] = metrics?.perTool ?? [];
  const costByTask: CostSlice[] = costs?.byTask ?? [];
  const costByModel: CostSlice[] = costs?.byModel ?? [];
  const costByTool: CostSlice[] = costs?.byTool ?? [];

  const cards: Array<{
    label: string;
    value: string;
    tone: PillTone;
    detail: string;
  }> = [
    {
      label: "Task success",
      value: metrics ? pct(metrics.successRate) : "—",
      tone: "ok",
      detail: metrics
        ? `${metrics.completedTasks}/${metrics.totalTasks} completed · ${metrics.failedTasks} failed`
        : "",
    },
    {
      label: "Policy denials",
      value: metrics ? pct(metrics.policyDenialRate) : "—",
      tone: metrics && metrics.policyDenialRate > 0.2 ? "warn" : "info",
      detail: metrics
        ? `${metrics.policyDenials}/${metrics.policyEvaluations} evaluations denied`
        : "",
    },
    {
      label: "Model latency p95",
      value: metrics ? `${metrics.modelLatency.p95Ms}ms` : "—",
      tone: "info",
      detail: metrics
        ? `p50 ${metrics.modelLatency.p50Ms}ms · p99 ${metrics.modelLatency.p99Ms}ms · ${metrics.modelLatency.samples} calls`
        : "",
    },
    {
      label: "Session spend",
      value: costs ? usd(costs.sessionTotalUsd) : "—",
      tone:
        costs && costs.sessionBudgetUsd !== null && costs.budgetConsumed >= 0.8
          ? "warn"
          : "info",
      detail:
        costs && costs.sessionBudgetUsd !== null
          ? `${pct(costs.budgetConsumed)} of ${usd(costs.sessionBudgetUsd)} budget`
          : "No session cap (metering only)",
    },
  ];

  return (
    <div className="flex flex-col gap-4">
      <SectionHeader
        title="Operations"
        subtitle="Local-first observability — traces, real cost attribution, and budget enforcement"
        action={
          <div className="flex gap-2">
            <NeonButton
              onClick={() => {
                void refresh();
              }}
              loading={loading}
            >
              Refresh
            </NeonButton>
            <NeonButton
              onClick={() => {
                void handleExport();
              }}
              loading={busy}
            >
              Export traces
            </NeonButton>
          </div>
        }
      />
      {notice !== null ? (
        <GlassPanel label="Notice">
          <p className="text-sm text-slate-300">{notice}</p>
        </GlassPanel>
      ) : null}
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 2xl:grid-cols-4">
        {cards.map((card) => (
          <GlassPanel key={card.label} label={card.label}>
            <p className="text-xs uppercase tracking-wider text-slate-500">
              {card.label}
            </p>
            <div className="mt-2 flex items-center justify-between">
              <span className="font-display text-2xl font-semibold">
                {card.value}
              </span>
              <StatusPill tone={card.tone}>{card.value}</StatusPill>
            </div>
            <p className="mt-2 text-xs text-slate-500">{card.detail}</p>
          </GlassPanel>
        ))}
      </div>

      <GlassPanel label="Session budget">
        <SectionHeader
          title="Session budget"
          subtitle="Global spend cap — live tool and model actions deny past the cap, warn at 80%. Empty clears the cap."
        />
        <div className="flex flex-wrap items-center gap-2">
          <input
            aria-label="Session budget in USD"
            className="w-48 rounded-md border border-white/10 bg-black/30 px-3 py-2 font-mono text-sm text-slate-200"
            placeholder="e.g. 5.00"
            value={budgetInput}
            onChange={(event) => {
              setBudgetInput(event.target.value);
            }}
          />
          <NeonButton
            onClick={() => {
              void handleBudget();
            }}
            loading={busy}
          >
            Set budget
          </NeonButton>
          {costs?.sessionBudgetUsd != null ? (
            <div
              className="h-2 min-w-48 flex-1 overflow-hidden rounded-full bg-white/5"
              role="img"
              aria-label={`Budget consumed ${pct(costs.budgetConsumed)}`}
            >
              <div
                className="h-full rounded-full bg-amber-400/80"
                style={{ width: `${costs.budgetConsumed * 100}%` }}
              />
            </div>
          ) : null}
        </div>
      </GlassPanel>

      <div className="grid grid-cols-1 gap-4 2xl:grid-cols-3">
        <GlassPanel label="Cost by task">
          <SectionHeader
            title="Cost by task"
            subtitle="LLM + tracked spend per task"
          />
          <CostBars rows={costByTask} unit="records" />
        </GlassPanel>
        <GlassPanel label="Cost by model">
          <SectionHeader
            title="Cost by model"
            subtitle="provider/model attribution"
          />
          <CostBars rows={costByModel} unit="calls" />
        </GlassPanel>
        <GlassPanel label="Cost by tool">
          <SectionHeader
            title="Cost by tool"
            subtitle="Tool and resource attribution"
          />
          <CostBars rows={costByTool} unit="records" />
        </GlassPanel>
      </div>

      <GlassPanel label="Model latency">
        <SectionHeader
          title="Model latency"
          subtitle={`Avg task ${metrics ? metrics.avgTaskDurationMs.toFixed(0) : "—"}ms · chain verify ${metrics ? metrics.chainVerifyMs.toFixed(1) : "—"}ms (${metrics?.chainChecked ?? 0} links)`}
        />
        <DataTable
          rows={perModel}
          keyOf={(row) => `${row.providerId}/${row.modelId}`}
          caption="Per-model latency and cost"
          columns={[
            {
              header: "Model",
              render: (row) => `${row.providerId}/${row.modelId}`,
            },
            { header: "Calls", render: (row) => String(row.calls) },
            { header: "p50", render: (row) => `${row.p50Ms}ms` },
            { header: "p95", render: (row) => `${row.p95Ms}ms` },
            { header: "p99", render: (row) => `${row.p99Ms}ms` },
            { header: "Cost", render: (row) => usd(row.costUsd) },
          ]}
        />
      </GlassPanel>

      <GlassPanel label="Tool health">
        <SectionHeader
          title="Tool health"
          subtitle="Success and failure per tool"
        />
        <DataTable
          rows={perTool}
          keyOf={(row) => row.toolId}
          caption="Per-tool success rates"
          columns={[
            { header: "Tool", render: (row) => row.toolId },
            { header: "Success", render: (row) => String(row.success) },
            { header: "Failure", render: (row) => String(row.failure) },
            { header: "Rate", render: (row) => pct(row.successRate) },
          ]}
        />
      </GlassPanel>

      {(metrics?.failureReasons.length ?? 0) > 0 ? (
        <GlassPanel label="Failure reasons">
          <SectionHeader
            title="Failure reasons"
            subtitle="Top last-error values for failed and timed-out tasks"
          />
          <DataTable
            rows={metrics?.failureReasons ?? []}
            keyOf={(row) => row.reason}
            caption="Top task failure reasons"
            columns={[
              { header: "Reason", render: (row) => row.reason },
              { header: "Tasks", render: (row) => String(row.count) },
            ]}
          />
        </GlassPanel>
      ) : null}

      <GlassPanel label="Recent traces">
        <SectionHeader
          title="Recent traces"
          subtitle="Span name, duration, status — metadata only, never prompts or screenshots"
        />
        <DataTable
          rows={traces}
          keyOf={(row, index) => `${row.name}-${row.startTime}-${index}`}
          caption="Recent trace spans"
          columns={[
            { header: "Span", render: (row) => row.name },
            { header: "Duration", render: (row) => `${row.durationMs}ms` },
            {
              header: "Status",
              render: (row) => (
                <StatusPill tone={row.status === "ok" ? "ok" : "error"}>
                  {row.statusMessage || row.status}
                </StatusPill>
              ),
            },
            { header: "Task", render: (row) => row.taskId ?? "—" },
          ]}
        />
      </GlassPanel>
    </div>
  );
}
