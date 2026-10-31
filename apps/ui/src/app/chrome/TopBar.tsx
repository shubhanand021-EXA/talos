import { useAppStore } from "../../stores/appStore.js";
import { useUiStore } from "../../stores/uiStore.js";
import { StatusPill } from "../../design-system/StatusPill.js";
import { Tooltip } from "../../design-system/Tooltip.js";

export function TopBar() {
  const { appInfo, systemStatus, connectionState } = useAppStore();
  const setPaletteOpen = useUiStore((state) => state.setCommandPaletteOpen);
  const healthy =
    systemStatus.policyBrokerHealthy && systemStatus.evidenceLedgerHealthy;

  return (
    <header
      aria-label="Application"
      className="flex h-14 shrink-0 items-center justify-between border-b border-white/10 bg-void-900/80 px-4 backdrop-blur"
    >
      <div className="flex items-center gap-3">
        <div
          aria-hidden="true"
          className="flex size-8 items-center justify-center rounded-lg border border-cyan-300/40 bg-cyan-400/10 font-display text-sm font-bold text-cyan-200 shadow-[0_0_24px_rgb(34_211_238/0.22)]"
        >
          T
        </div>
        <span className="font-display text-base font-semibold tracking-tight">
          {appInfo.name}
          <span className="neon-text-cyan"> OS</span>
        </span>
        <StatusPill tone={appInfo.environment === "local" ? "info" : "warn"}>
          {appInfo.environment}
        </StatusPill>
      </div>
      <div className="flex items-center gap-3">
        <StatusPill tone={healthy ? "ok" : "error"}>
          {healthy ? "systems nominal" : "attention required"}
        </StatusPill>
        <StatusPill tone={connectionState === "connected" ? "ok" : "warn"}>
          {connectionState}
        </StatusPill>
        <Tooltip label="Open command palette (Ctrl+K)">
          <button
            type="button"
            onClick={() => {
              setPaletteOpen(true);
            }}
            className="rounded-control border border-white/10 bg-white/5 px-3 py-1.5 font-mono text-xs text-slate-300 hover:border-cyan-300/40 hover:text-cyan-200"
          >
            ⌘K
          </button>
        </Tooltip>
      </div>
    </header>
  );
}
