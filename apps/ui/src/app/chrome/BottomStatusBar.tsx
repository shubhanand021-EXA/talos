import { useAppStore } from "../../stores/appStore.js";
import { StatusPill } from "../../design-system/StatusPill.js";
import { getMockAgents, getMockTasks } from "../../stores/mockStore.js";

export function BottomStatusBar() {
  const { systemStatus } = useAppStore();
  const agents = getMockAgents();
  const tasks = getMockTasks();
  const running = agents.filter((agent) => agent.status === "running").length;
  const active = tasks.filter(
    (task) => task.status === "executing" || task.status === "planning",
  ).length;

  return (
    <footer
      aria-label="System status"
      className="flex h-9 shrink-0 items-center justify-between gap-4 overflow-x-auto border-t border-white/10 bg-void-900/80 px-4 text-xs"
    >
      <div className="flex items-center gap-3">
        <StatusPill tone={systemStatus.policyBrokerHealthy ? "ok" : "error"}>
          {`policy broker ${systemStatus.policyBrokerHealthy ? "healthy" : "down"}`}
        </StatusPill>
        <StatusPill tone={systemStatus.evidenceLedgerHealthy ? "ok" : "error"}>
          {`evidence ledger ${systemStatus.evidenceLedgerHealthy ? "healthy" : "down"}`}
        </StatusPill>
      </div>
      <div className="flex items-center gap-3 text-slate-400">
        <span>
          {running} running · {active} active tasks
        </span>
        <span
          className="font-mono text-slate-600"
          title="3D viewport frame rate (not yet integrated)"
        >
          FPS ——
        </span>
      </div>
    </footer>
  );
}
