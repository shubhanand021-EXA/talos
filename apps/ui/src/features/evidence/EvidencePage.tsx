import { useCallback, useEffect, useState } from "react";
import {
  DataTable,
  EmptyState,
  GlassPanel,
  NeonButton,
  SectionHeader,
  Spinner,
  StatusPill,
} from "../../design-system/index.js";
import {
  isTauri,
  verifyEvidenceLedger,
  type ClaimSummary,
  type CompensationPlanSummary,
  type EventSummary,
  type EvidenceSummary,
  type LedgerStatus,
} from "../../bridge/index.js";
import { getMockActivity } from "../../stores/mockStore.js";
import { useAppStore } from "../../stores/appStore.js";
import { useUiStore } from "../../stores/uiStore.js";
import { formatTime, shortId } from "../../lib/format.js";

interface EvidenceRow {
  id: string;
  kind: string;
  status: string;
  label: string;
}

const ROWS: EvidenceRow[] = [
  {
    id: "ev-1041",
    kind: "command_result",
    status: "verified",
    label: "pnpm test — contracts",
  },
  {
    id: "ev-1042",
    kind: "file_hash",
    status: "pending",
    label: "packages/contracts/src diff",
  },
  {
    id: "ev-1043",
    kind: "test_result",
    status: "verified",
    label: "policy selftest 6/6",
  },
];

type LoadState =
  | { phase: "idle" }
  | { phase: "loading" }
  | { phase: "ready"; ledger: LedgerStatus }
  | { phase: "error"; message: string };

function toneFor(status: string): "ok" | "info" | "warn" | "error" | "neutral" {
  switch (status) {
    case "verified":
    case "done":
    case "executed":
    case "approved":
      return "ok";
    case "pending":
    case "proposed":
    case "in_progress":
      return "warn";
    case "failed":
    case "rejected":
    case "conflicting":
      return "error";
    default:
      return "neutral";
  }
}

function LiveEvidencePage() {
  const [state, setState] = useState<LoadState>({ phase: "loading" });
  const selectEntity = useUiStore((state) => state.selectEntity);
  const appendFeed = useAppStore((state) => state.appendFeed);
  const bumpEvidenceVersion = useAppStore((state) => state.bumpEvidenceVersion);

  const applyLedger = useCallback((ledger: LedgerStatus) => {
    setState({ phase: "ready", ledger });
    const store = useAppStore.getState();
    store.setSystemStatus({
      ...store.systemStatus,
      evidenceLedgerHealthy: ledger.healthy,
    });
  }, []);

  const applyError = useCallback((error: unknown) => {
    const message = error instanceof Error ? error.message : String(error);
    setState({ phase: "error", message });
  }, []);

  // Initial fetch runs once on mount. State updates happen in promise
  // continuations (never synchronously in the effect body).
  useEffect(() => {
    let cancelled = false;
    verifyEvidenceLedger().then(
      (ledger) => {
        if (!cancelled) {
          applyLedger(ledger);
        }
      },
      (error: unknown) => {
        if (!cancelled) {
          applyError(error);
        }
      },
    );
    return () => {
      cancelled = true;
    };
  }, [applyLedger, applyError]);

  const refreshWithSpinner = useCallback(async () => {
    setState({ phase: "loading" });
    try {
      const ledger = await verifyEvidenceLedger();
      applyLedger(ledger);
      return ledger;
    } catch (error) {
      applyError(error);
      return null;
    }
  }, [applyLedger, applyError]);

  const handleVerify = useCallback(async () => {
    const ledger = await refreshWithSpinner();
    if (ledger === null) {
      return;
    }
    bumpEvidenceVersion();
    appendFeed({
      id: `verify-${Date.now()}`,
      at: new Date().toISOString(),
      actor: "evidence-store",
      message: ledger.chain_valid
        ? `Ledger verified: ${ledger.chain_checked} links, seq ${ledger.chain_last_seq ?? 0}`
        : `Ledger INVALID at seq ${ledger.chain_first_bad_seq ?? "?"}`,
      tone: ledger.chain_valid ? "success" : "error",
    });
  }, [refreshWithSpinner, bumpEvidenceVersion, appendFeed]);

  const ledger = state.phase === "ready" ? state.ledger : null;

  return (
    <div className="flex flex-col gap-4">
      <SectionHeader
        title="Evidence"
        subtitle="Append-only ledger with hash-chained integrity"
        action={
          <div className="flex gap-2">
            <NeonButton
              variant="ghost"
              onClick={() => {
                void refreshWithSpinner();
              }}
              loading={state.phase === "loading"}
            >
              Refresh
            </NeonButton>
            <NeonButton
              onClick={() => {
                void handleVerify();
              }}
              loading={state.phase === "loading"}
            >
              Verify Ledger
            </NeonButton>
          </div>
        }
      />
      {state.phase === "loading" && ledger === null ? (
        <GlassPanel label="Loading ledger">
          <div className="flex items-center justify-center py-10">
            <Spinner label="Reading ledger" />
          </div>
        </GlassPanel>
      ) : null}
      {state.phase === "error" ? (
        <GlassPanel label="Ledger error">
          <EmptyState
            title="Ledger unreachable"
            message={state.message}
            action={
              <NeonButton
                onClick={() => {
                  void refreshWithSpinner();
                }}
              >
                Retry
              </NeonButton>
            }
          />
        </GlassPanel>
      ) : null}
      {ledger !== null ? (
        <>
          <GlassPanel label="Integrity">
            <div className="flex flex-wrap items-center gap-3">
              <StatusPill tone={ledger.chain_valid ? "ok" : "error"}>
                {ledger.chain_valid ? "chain valid" : "CHAIN BROKEN"}
              </StatusPill>
              <span className="font-mono text-xs text-slate-400">
                {ledger.chain_checked} links checked
              </span>
              {ledger.chain_last_seq !== null &&
              ledger.chain_last_seq !== undefined ? (
                <span className="font-mono text-xs text-slate-400">
                  last seq {ledger.chain_last_seq}
                </span>
              ) : null}
              {ledger.chain_last_hash != null ? (
                <span
                  className="font-mono text-xs text-slate-400"
                  title={ledger.chain_last_hash}
                >
                  head {shortId(ledger.chain_last_hash, 16)}
                </span>
              ) : null}
              {ledger.chain_first_bad_seq !== null &&
              ledger.chain_first_bad_seq !== undefined ? (
                <StatusPill tone="error">
                  {`first bad seq ${ledger.chain_first_bad_seq}`}
                </StatusPill>
              ) : null}
            </div>
            <p className="mt-2 text-xs text-slate-500">{ledger.detail}</p>
          </GlassPanel>
          <LedgerTable<EventSummary>
            title="Chained events"
            caption="Hash-chained event log, newest first"
            rows={ledger.recent_events}
            emptyTitle="No chained events yet"
            emptyMessage="Run a diagnostic to append the first linked events."
            columns={[
              {
                header: "Seq",
                render: (row) => (
                  <span className="font-mono text-xs">{row.seq}</span>
                ),
              },
              { header: "Kind", render: (row) => row.kind },
              {
                header: "ID",
                render: (row) => (
                  <span className="font-mono text-xs" title={row.id}>
                    {shortId(row.id)}
                  </span>
                ),
              },
              { header: "At", render: (row) => formatTime(row.created_at) },
            ]}
            keyOf={(row) => `${row.seq}-${row.id}`}
            onSelect={(row) => {
              selectEntity({
                kind: "system",
                id: row.id,
                label: `Event #${row.seq} ${row.kind}`,
                detail: `Hash-chained at ${row.created_at}`,
              });
            }}
          />
          <LedgerTable<EvidenceSummary>
            title="Evidence records"
            caption="Evidence rows with payload hashes, newest first"
            rows={ledger.recent_evidence}
            emptyTitle="No evidence yet"
            emptyMessage="Run a diagnostic to record the first evidence."
            columns={[
              {
                header: "ID",
                render: (row) => (
                  <span className="font-mono text-xs" title={row.id}>
                    {shortId(row.id)}
                  </span>
                ),
              },
              { header: "Kind", render: (row) => row.kind },
              {
                header: "Status",
                render: (row) => (
                  <StatusPill tone={toneFor(row.status)}>
                    {row.status}
                  </StatusPill>
                ),
              },
              {
                header: "Hash",
                render: (row) => (
                  <span className="font-mono text-xs" title={row.hash}>
                    {shortId(row.hash, 16)}
                  </span>
                ),
              },
              { header: "At", render: (row) => formatTime(row.created_at) },
            ]}
            keyOf={(row) => row.id}
            onSelect={(row) => {
              selectEntity({
                kind: "evidence",
                id: row.id,
                label: row.id,
                detail: `${row.kind} · ${row.status} · hash ${shortId(row.hash, 16)}`,
              });
            }}
          />
          <LedgerTable<ClaimSummary>
            title="Claims"
            caption="Claim lifecycle: pending, verified, rejected, conflicting"
            rows={ledger.recent_claims}
            emptyTitle="No claims yet"
            emptyMessage="Claims appear here once the runtime raises them."
            columns={[
              {
                header: "ID",
                render: (row) => (
                  <span className="font-mono text-xs" title={row.id}>
                    {shortId(row.id)}
                  </span>
                ),
              },
              { header: "Statement", render: (row) => row.statement },
              {
                header: "Status",
                render: (row) => (
                  <StatusPill tone={toneFor(row.status)}>
                    {row.status}
                  </StatusPill>
                ),
              },
              { header: "At", render: (row) => formatTime(row.created_at) },
            ]}
            keyOf={(row) => row.id}
            onSelect={(row) => {
              selectEntity({
                kind: "system",
                id: row.id,
                label: `Claim ${shortId(row.id)}`,
                detail: `${row.statement} — ${row.status}`,
              });
            }}
          />
          <LedgerTable<CompensationPlanSummary>
            title="Compensation plans"
            caption="Plans covering irreversible actions"
            rows={ledger.compensation_plans}
            emptyTitle="No compensation plans"
            emptyMessage="Plans are created automatically when irreversible actions are proposed."
            columns={[
              {
                header: "ID",
                render: (row) => (
                  <span className="font-mono text-xs" title={row.id}>
                    {shortId(row.id)}
                  </span>
                ),
              },
              { header: "Strategy", render: (row) => row.strategy },
              {
                header: "Status",
                render: (row) => (
                  <StatusPill tone={toneFor(row.status)}>
                    {row.status}
                  </StatusPill>
                ),
              },
              {
                header: "Updated",
                render: (row) => formatTime(row.updated_at),
              },
            ]}
            keyOf={(row) => row.id}
            onSelect={(row) => {
              selectEntity({
                kind: "system",
                id: row.id,
                label: `Plan ${shortId(row.id)}`,
                detail: `${row.strategy} · ${row.status} · covers ${shortId(row.action_request_id)}`,
              });
            }}
          />
        </>
      ) : null}
    </div>
  );
}

interface LedgerTableProps<T> {
  title: string;
  caption: string;
  rows: T[];
  emptyTitle: string;
  emptyMessage: string;
  columns: Array<{ header: string; render: (row: T) => React.ReactNode }>;
  keyOf: (row: T) => string;
  onSelect: (row: T) => void;
}

function LedgerTable<T>({
  title,
  caption,
  rows,
  emptyTitle,
  emptyMessage,
  columns,
  keyOf,
  onSelect,
}: LedgerTableProps<T>) {
  return (
    <GlassPanel label={title}>
      <SectionHeader title={title} subtitle={`${rows.length} rows`} />
      {rows.length === 0 ? (
        <EmptyState title={emptyTitle} message={emptyMessage} />
      ) : (
        <DataTable<T>
          caption={caption}
          rows={rows}
          keyOf={(_, index) => `${keyOf(rows[index] as T)}-${index}`}
          onRowSelect={onSelect}
          columns={columns}
        />
      )}
    </GlassPanel>
  );
}

export function EvidencePage() {
  if (!isTauri()) {
    return <MockEvidencePage />;
  }
  return <LiveEvidencePage />;
}

function MockEvidencePage() {
  const activity = getMockActivity();
  const selectEntity = useUiStore((state) => state.selectEntity);

  return (
    <div className="flex flex-col gap-4">
      <SectionHeader
        title="Evidence"
        subtitle="Ledger records backing every claim — mock snapshot (launch the desktop shell for live data)"
      />
      <GlassPanel label="Evidence records">
        <DataTable<EvidenceRow>
          caption="Evidence records"
          rows={ROWS}
          keyOf={(row) => row.id}
          onRowSelect={(row) => {
            selectEntity({
              kind: "evidence",
              id: row.id,
              label: row.label,
              detail: `${row.kind} · ${row.status}`,
            });
          }}
          columns={[
            {
              header: "ID",
              render: (row) => (
                <span className="font-mono text-xs">{row.id}</span>
              ),
            },
            { header: "Kind", render: (row) => row.kind },
            {
              header: "Status",
              render: (row) => (
                <StatusPill tone={row.status === "verified" ? "ok" : "warn"}>
                  {row.status}
                </StatusPill>
              ),
            },
            { header: "Label", render: (row) => row.label },
          ]}
        />
      </GlassPanel>
      <GlassPanel label="Claims">
        <SectionHeader
          title="Open claims"
          subtitle={`${activity.length} linked events`}
        />
        <EmptyState
          title="claim-17 under review"
          message="“Tests pass on clean machine” — 1 of 1 evidence records verified. Full claim workflow arrives with the ledger UI."
        />
      </GlassPanel>
    </div>
  );
}
