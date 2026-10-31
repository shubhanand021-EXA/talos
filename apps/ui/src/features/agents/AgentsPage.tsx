import type { AgentDescriptor } from "@talos/contracts";
import {
  DataTable,
  GlassPanel,
  SectionHeader,
  StatusPill,
  type PillTone,
} from "../../design-system/index.js";
import { getMockAgents } from "../../stores/mockStore.js";
import { useUiStore } from "../../stores/uiStore.js";

const STATUS_TONE: Record<AgentDescriptor["status"], PillTone> = {
  idle: "neutral",
  starting: "info",
  running: "ok",
  waiting_policy: "warn",
  failed: "error",
  terminated: "neutral",
  completed: "ok",
};

export function AgentsPage() {
  const agents = getMockAgents();
  const selectEntity = useUiStore((state) => state.selectEntity);

  return (
    <div className="flex flex-col gap-4">
      <SectionHeader
        title="Agents"
        subtitle={`${agents.length} configured · deterministic mock roster`}
      />
      <GlassPanel label="Agent roster">
        <DataTable<AgentDescriptor>
          caption="Agent roster"
          rows={agents}
          keyOf={(row) => row.id}
          onRowSelect={(row) => {
            selectEntity({
              kind: "agent",
              id: row.id,
              label: row.displayName,
              detail: `${row.role} · ${row.status} · ${row.capabilities.join(", ")}`,
            });
          }}
          columns={[
            { header: "Agent", render: (row) => row.displayName },
            {
              header: "Role",
              render: (row) => (
                <span className="font-mono text-xs">{row.role}</span>
              ),
            },
            {
              header: "Status",
              render: (row) => (
                <StatusPill tone={STATUS_TONE[row.status]}>
                  {row.status}
                </StatusPill>
              ),
            },
            {
              header: "Capabilities",
              render: (row) => row.capabilities.join(", "),
            },
          ]}
        />
      </GlassPanel>
    </div>
  );
}
