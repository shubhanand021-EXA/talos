import { useEffect, useState } from "react";
import type {
  ContextBudgetStatus,
  ContextCompactionReceipt,
} from "@talos/contracts";
import {
  DataTable,
  EmptyState,
  GlassPanel,
  KeyValueList,
  NeonButton,
  SectionHeader,
  StatusPill,
  type PillTone,
} from "../../design-system/index.js";
import {
  compactDiagnosticContext,
  compactTaskContext,
  getContextBudgetStatus,
  isBackendUnavailable,
} from "../../bridge/index.js";
import { useAppStore } from "../../stores/appStore.js";
import { useUiStore } from "../../stores/uiStore.js";

const LAYER_TONE: Record<string, PillTone> = {
  safety: "ok",
  system: "ok",
  task: "info",
  session: "info",
  evidence: "warn",
  tool: "warn",
  scratchpad: "neutral",
};

const LAYER_ORDER = [
  "safety",
  "system",
  "task",
  "session",
  "evidence",
  "tool",
  "scratchpad",
] as const;

const LAYER_BLURB: Record<string, string> = {
  safety: "Safety constraints — protected, always verbatim.",
  system: "Product instructions — protected, verbatim.",
  task: "Task intent and lifecycle — protected, verbatim.",
  session: "Session identity — protected, verbatim.",
  evidence: "Ledger evidence references — trimmed, never dropped with refs.",
  tool: "Tool outputs and receipts — droppable when reference-free.",
  scratchpad: "Working notes — first to trim or drop under pressure.",
};

function LayerBars({ status }: { status: ContextBudgetStatus }) {
  const ordered = [...status.layers].sort(
    (a, b) => LAYER_ORDER.indexOf(a.layer) - LAYER_ORDER.indexOf(b.layer),
  );
  return (
    <div className="flex flex-col gap-2">
      {ordered.map((layer) => {
        const pct =
          layer.maxTokens === 0
            ? 0
            : Math.min(100, (layer.usedTokens / layer.maxTokens) * 100);
        return (
          <div key={layer.layer} className="flex flex-col gap-1">
            <div className="flex items-center justify-between gap-2 text-xs">
              <span className="flex items-center gap-2">
                <StatusPill tone={LAYER_TONE[layer.layer] ?? "neutral"}>
                  {layer.layer}
                </StatusPill>
                {layer.protected ? (
                  <span className="text-slate-500">protected</span>
                ) : null}
                {layer.overflow ? (
                  <StatusPill tone="error">overflow</StatusPill>
                ) : null}
              </span>
              <span className="font-mono text-slate-400">
                {layer.usedTokens}/{layer.maxTokens} · {layer.segmentCount} segs
              </span>
            </div>
            <div
              role="progressbar"
              aria-label={`${layer.layer} budget`}
              aria-valuenow={Math.round(pct)}
              aria-valuemin={0}
              aria-valuemax={100}
              className="h-1.5 overflow-hidden rounded-full bg-white/10"
            >
              <div
                className={`h-full rounded-full ${layer.overflow ? "bg-red-400" : "bg-cyan-400"}`}
                style={{ width: `${pct}%` }}
              />
            </div>
            <p className="text-xs text-slate-500">
              {LAYER_BLURB[layer.layer] ?? ""}
            </p>
          </div>
        );
      })}
    </div>
  );
}

export function ContextPage() {
  const selectEntity = useUiStore((state) => state.selectEntity);
  const lastContextReceipt = useAppStore((state) => state.lastContextReceipt);
  const setLastContextReceipt = useAppStore(
    (state) => state.setLastContextReceipt,
  );
  const [taskId, setTaskId] = useState("task-1");
  const [totalBudget, setTotalBudget] = useState("8000");
  const [status, setStatus] = useState<ContextBudgetStatus | null>(null);
  const [receipt, setReceipt] = useState<ContextCompactionReceipt | null>(
    lastContextReceipt,
  );
  const [loading, setLoading] = useState(false);
  const [compacting, setCompacting] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);

  const budgetNumber = (): number | undefined => {
    const parsed = Number.parseInt(totalBudget, 10);
    return Number.isFinite(parsed) && parsed > 0 ? parsed : undefined;
  };

  const refreshStatus = async () => {
    setLoading(true);
    setNotice(null);
    try {
      const next = await getContextBudgetStatus(taskId, budgetNumber());
      setStatus(next);
    } catch (error) {
      setNotice(
        isBackendUnavailable(error)
          ? "Backend unavailable — launch the desktop shell."
          : error instanceof Error
            ? error.message
            : "Budget load failed",
      );
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      setLoading(true);
      setNotice(null);
      try {
        const next = await getContextBudgetStatus(taskId, budgetNumber());
        if (!cancelled) {
          setStatus(next);
        }
      } catch (error) {
        if (!cancelled) {
          setNotice(
            isBackendUnavailable(error)
              ? "Backend unavailable — launch the desktop shell."
              : error instanceof Error
                ? error.message
                : "Budget load failed",
          );
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
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const runCompact = async (diagnostic: boolean) => {
    setCompacting(true);
    setNotice(null);
    try {
      const next = diagnostic
        ? await compactDiagnosticContext(budgetNumber())
        : await compactTaskContext(taskId, budgetNumber());
      setReceipt(next);
      setLastContextReceipt(next);
      selectEntity({
        kind: "system",
        id: next.id,
        label: `TypeCompact ${next.originalTokens} → ${next.compactedTokens}`,
        detail: `Proof ${next.proofHash}. ${next.preservedInvariants.join(", ")}.`,
      });
      await refreshStatus();
    } catch (error) {
      setNotice(
        isBackendUnavailable(error)
          ? "Backend unavailable — launch the desktop shell."
          : error instanceof Error
            ? error.message
            : "Compaction failed",
      );
    } finally {
      setCompacting(false);
    }
  };

  const protectedItems =
    receipt?.segments.filter((segment) => segment.protected) ?? [];
  const previewSegments =
    receipt?.segments.filter((segment) => !segment.dropped).slice(0, 8) ?? [];

  return (
    <div className="flex flex-col gap-4">
      <SectionHeader
        title="Context"
        subtitle={
          loading
            ? "Loading budget…"
            : status === null
              ? "TypeCompact budget engine and compaction foundation"
              : `${status.totalUsed}/${status.totalBudget} tokens · ${status.overflow ? "overflow" : "within budget"}`
        }
        action={
          <div className="flex gap-2">
            <NeonButton
              onClick={() => {
                void refreshStatus();
              }}
              loading={loading}
            >
              Refresh budget
            </NeonButton>
            <NeonButton
              onClick={() => {
                void runCompact(false);
              }}
              loading={compacting}
            >
              Compact
            </NeonButton>
            <NeonButton
              onClick={() => {
                void runCompact(true);
              }}
              loading={compacting}
            >
              Compact diagnostic
            </NeonButton>
          </div>
        }
      />
      {notice !== null ? (
        <p role="status" className="text-xs text-amber-300">
          {notice}
        </p>
      ) : null}
      <GlassPanel label="Task">
        <div className="flex flex-col gap-2">
          <label className="text-xs text-slate-400" htmlFor="context-task">
            Task id (ledger linkage)
          </label>
          <input
            id="context-task"
            value={taskId}
            onChange={(event) => {
              setTaskId(event.target.value);
            }}
            className="rounded-md border border-white/10 bg-black/30 px-2 py-1 text-sm"
          />
          <label className="text-xs text-slate-400" htmlFor="context-budget">
            Total budget (tokens, default 8000)
          </label>
          <input
            id="context-budget"
            value={totalBudget}
            onChange={(event) => {
              setTotalBudget(event.target.value);
            }}
            inputMode="numeric"
            className="rounded-md border border-white/10 bg-black/30 px-2 py-1 text-sm"
          />
          <p className="text-xs text-slate-500">
            Seven layers: safety, system, task, and session are protected and
            verbatim; evidence trims but keeps every reference; tool and
            scratchpad absorb overflow first. TypeCompact never silently drops
            safety-critical fields.
          </p>
        </div>
      </GlassPanel>
      <GlassPanel label="Budget">
        {status === null ? (
          <EmptyState
            title="No budget yet"
            message="Launch the desktop shell, then refresh to read the live budget."
          />
        ) : (
          <div className="flex flex-col gap-3">
            <div className="flex flex-wrap items-center gap-2">
              <StatusPill tone={status.overflow ? "error" : "ok"}>
                {status.overflow ? "overflow" : "within budget"}
              </StatusPill>
              <StatusPill tone="info">
                {`${status.totalUsed}/${status.totalBudget} tokens`}
              </StatusPill>
            </div>
            <LayerBars status={status} />
          </div>
        )}
      </GlassPanel>
      <GlassPanel label="Compacted preview">
        {receipt === null ? (
          <EmptyState
            title="No compaction yet"
            message="Compact this task (or the diagnostic snapshot) to preview the TypeCompact output."
          />
        ) : (
          <div className="flex flex-col gap-2">
            <div className="flex flex-wrap items-center gap-2">
              <StatusPill tone="info">
                {`${receipt.originalTokens} → ${receipt.compactedTokens} tokens`}
              </StatusPill>
              {receipt.droppedSegments.length > 0 ? (
                <StatusPill tone="warn">
                  {`dropped ${receipt.droppedSegments.length}`}
                </StatusPill>
              ) : (
                <StatusPill tone="ok">nothing dropped</StatusPill>
              )}
            </div>
            {previewSegments.map((segment) => (
              <div
                key={segment.id}
                className="rounded-md border border-white/10 bg-black/30 p-2"
              >
                <div className="flex items-center justify-between gap-2 text-xs">
                  <span className="font-mono text-slate-300">
                    {segment.id} · {segment.layer}
                  </span>
                  <span className="flex gap-1">
                    {segment.protected ? (
                      <StatusPill tone="ok">protected</StatusPill>
                    ) : null}
                    {segment.trimmed ? (
                      <StatusPill tone="warn">trimmed</StatusPill>
                    ) : null}
                  </span>
                </div>
                <p className="mt-1 max-h-20 overflow-auto whitespace-pre-wrap text-xs text-slate-400">
                  {segment.body.length > 400
                    ? `${segment.body.slice(0, 400)}…`
                    : segment.body}
                </p>
                <p className="mt-1 font-mono text-xs text-slate-500">
                  {segment.originalTokens} → {segment.compactedTokens} tokens
                </p>
              </div>
            ))}
            {receipt.segments.length > previewSegments.length ? (
              <p className="text-xs text-slate-500">
                Showing {previewSegments.length} of {receipt.segments.length}{" "}
                segments.
              </p>
            ) : null}
          </div>
        )}
      </GlassPanel>
      <GlassPanel label="Invariant checklist">
        {receipt === null ? (
          <EmptyState
            title="No invariants yet"
            message="Invariants are attested on every compaction and rejected on any loss."
          />
        ) : (
          <div className="flex flex-wrap gap-2">
            {receipt.preservedInvariants.map((invariant) => (
              <StatusPill key={invariant} tone="ok">
                {invariant}
              </StatusPill>
            ))}
          </div>
        )}
      </GlassPanel>
      <div className="grid grid-cols-1 gap-4 2xl:grid-cols-2">
        <GlassPanel label="Compaction receipt">
          {receipt === null ? (
            <EmptyState
              title="No receipt yet"
              message="Compact to persist a proof-hashed receipt as ledger evidence."
            />
          ) : (
            <KeyValueList
              entries={[
                { key: "receipt", value: receipt.id },
                { key: "proof hash", value: receipt.proofHash },
                {
                  key: "tokens",
                  value: `${receipt.originalTokens} → ${receipt.compactedTokens}`,
                },
                { key: "evidence", value: receipt.evidenceId ?? "—" },
                { key: "version", value: receipt.version },
                { key: "executed", value: receipt.executedAt },
                {
                  key: "dropped",
                  value:
                    receipt.droppedSegments.length > 0
                      ? receipt.droppedSegments.join(", ")
                      : "none",
                },
                {
                  key: "trimmed",
                  value:
                    receipt.trimmedSegments.length > 0
                      ? receipt.trimmedSegments.join(", ")
                      : "none",
                },
              ]}
            />
          )}
        </GlassPanel>
        <GlassPanel label="Protected items">
          {receipt === null ? (
            <EmptyState
              title="No protected items yet"
              message="Protected segments are carried verbatim through every compaction."
            />
          ) : (
            <DataTable
              caption="Protected segments"
              rows={protectedItems.map((segment) => ({
                id: segment.id,
                layer: segment.layer,
                tokens: `${segment.originalTokens} → ${segment.compactedTokens}`,
              }))}
              keyOf={(row) => row.id}
              columns={[
                { header: "Segment", render: (row) => row.id },
                { header: "Layer", render: (row) => row.layer },
                { header: "Tokens", render: (row) => row.tokens },
              ]}
            />
          )}
        </GlassPanel>
      </div>
    </div>
  );
}
