import { useAppStore } from "../../stores/appStore.js";
import { useApprovalStore } from "../../stores/approvalStore.js";
import { useUiStore } from "../../stores/uiStore.js";
import { StatusPill } from "../../design-system/StatusPill.js";

export function BottomStatusBar() {
  const { systemStatus, computerSafety, liveTasks } = useAppStore();
  const viewportFps = useUiStore((state) => state.viewportFps);
  const pendingCount = useApprovalStore((state) => state.pending.length);
  const openSheet = useApprovalStore((state) => state.openSheet);
  const waitingPolicy = liveTasks.filter(
    (task) => task.status === "waiting_policy",
  ).length;
  const approvals = Math.max(
    pendingCount,
    waitingPolicy > 0 && pendingCount === 0 ? waitingPolicy : pendingCount,
  );

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
        {waitingPolicy > 0 ? (
          <StatusPill tone="warn">{`policy gate ${waitingPolicy}`}</StatusPill>
        ) : null}
        {approvals > 0 ? (
          <button
            type="button"
            onClick={() => {
              openSheet();
            }}
            aria-label={`${approvals} approvals waiting, open approval sheet`}
            className="approval-pulse rounded-full border border-amber-400/40 bg-amber-400/10 px-2 py-0.5 font-semibold text-amber-200 motion-reduce:animate-none"
          >
            <span
              className="approval-pulse-dot mr-1 inline-block h-2 w-2 rounded-full bg-amber-400 motion-reduce:animate-none"
              aria-hidden="true"
            />
            {`${approvals} approval${approvals === 1 ? "" : "s"} waiting`}
          </button>
        ) : null}
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
