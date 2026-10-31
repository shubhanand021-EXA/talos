import { useAppStore } from "../../stores/appStore.js";
import {
  getMockActivity,
  getMockAgents,
  getMockTasks,
} from "../../stores/mockStore.js";
import {
  EmptyState,
  GlassPanel,
  LogStream,
  SectionHeader,
  StatusPill,
  type PillTone,
} from "../../design-system/index.js";
import { formatUptime } from "../../lib/format.js";

export function DashboardPage() {
  const { systemStatus } = useAppStore();
  const agents = getMockAgents();
  const tasks = getMockTasks();
  const activity = getMockActivity();
  const running = agents.filter((agent) => agent.status === "running").length;

  const cards: Array<{
    label: string;
    value: string;
    tone: PillTone;
    detail: string;
  }> = [
    {
      label: "Policy broker",
      value: systemStatus.policyBrokerHealthy ? "Healthy" : "Down",
      tone: systemStatus.policyBrokerHealthy ? "ok" : "error",
      detail: "Default-deny engine · 6 rules",
    },
    {
      label: "Evidence ledger",
      value: systemStatus.evidenceLedgerHealthy ? "Healthy" : "Down",
      tone: systemStatus.evidenceLedgerHealthy ? "ok" : "error",
      detail: "SQLite store · append-only",
    },
    {
      label: "Active agents",
      value: String(running),
      tone: "info",
      detail: `${agents.length} configured`,
    },
    {
      label: "Active tasks",
      value: String(tasks.length),
      tone: "info",
      detail: `Uptime ${formatUptime(systemStatus.uptimeSeconds)}`,
    },
  ];

  return (
    <div className="flex flex-col gap-4">
      <SectionHeader
        title="Dashboard"
        subtitle="Fleet overview — deterministic mock snapshot for Batch 1"
      />
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
      <div className="grid grid-cols-1 gap-4 2xl:grid-cols-2">
        <GlassPanel label="Recent activity">
          <SectionHeader title="Recent activity" subtitle="Newest first" />
          <LogStream events={activity} />
        </GlassPanel>
        <GlassPanel label="Spatial viewport">
          <EmptyState
            title="3D viewport coming online"
            message="The Void Neon spatial topology docks here in a later batch. Agent graph state will stream from the policy broker."
          />
        </GlassPanel>
      </div>
    </div>
  );
}
