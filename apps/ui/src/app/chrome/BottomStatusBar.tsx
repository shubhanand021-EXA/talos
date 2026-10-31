import { useAppStore } from "../../stores/appStore.js";
import { useUiStore } from "../../stores/uiStore.js";
import { StatusPill } from "../../design-system/StatusPill.js";

export function BottomStatusBar() {
  const { systemStatus, computerSafety } = useAppStore();
  const viewportFps = useUiStore((state) => state.viewportFps);

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
        {computerSafety !== null && computerSafety.emergencyStopped ? (
          <StatusPill tone="error">computer-use STOPPED</StatusPill>
        ) : (
          <StatusPill tone="neutral">
            {`computer-use ${computerSafety === null ? "unknown" : computerSafety.enabled ? "armed" : "disabled"}`}
          </StatusPill>
        )}
      </div>
      <div className="flex items-center gap-3 text-slate-400">
        <span>
          {systemStatus.activeAgents} agents · {systemStatus.activeTasks} active
          tasks
        </span>
        <span
          className="font-mono text-slate-400"
          title="Live frame rate from the Void Neon viewport"
        >
          {viewportFps > 0 ? `FPS ${viewportFps}` : "FPS —"}
        </span>
      </div>
    </footer>
  );
}
