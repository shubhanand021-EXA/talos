import {
  DataTable,
  EmptyState,
  GlassPanel,
  SectionHeader,
  StatusPill,
} from "../../design-system/index.js";
import { getMockActivity } from "../../stores/mockStore.js";
import { useUiStore } from "../../stores/uiStore.js";

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

export function EvidencePage() {
  const activity = getMockActivity();
  const selectEntity = useUiStore((state) => state.selectEntity);

  return (
    <div className="flex flex-col gap-4">
      <SectionHeader
        title="Evidence"
        subtitle="Ledger records backing every claim — mock snapshot"
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
