import { useGatewayStatus, useRestartGateway } from "./useGatewayStatus.js";

/**
 * Daemon link banner. Rendered above the shell when the gateway is
 * unreachable: red DISCONNECTED state + endpoint + Restart Daemon button
 * (wired to the Tauri `gateway_restart` command, which spawns a fresh
 * detached daemon and re-probes). Hidden while connected/connecting.
 */
export function GatewayStatusBanner() {
  const status = useGatewayStatus();
  const { restarting, restart } = useRestartGateway();

  if (status.link !== "disconnected") {
    return null;
  }

  return (
    <div
      role="alert"
      aria-label="Gateway disconnected"
      className="flex shrink-0 items-center justify-between gap-4 border-b border-red-500/40 bg-red-950/80 px-4 py-2 text-sm text-red-100"
    >
      <span>
        <strong className="font-semibold">DISCONNECTED</strong>
        <span className="ml-2 text-red-200/80">
          {`Daemon unreachable at ${status.url} (${status.missedPongs} missed heartbeats). Tasks and ledger writes are paused here — the daemon keeps running headless.`}
        </span>
      </span>
      <button
        type="button"
        onClick={() => {
          void restart();
        }}
        disabled={restarting}
        aria-label="Restart daemon"
        className="shrink-0 rounded-md border border-red-400/50 bg-red-500/20 px-3 py-1 font-semibold text-red-50 hover:bg-red-500/30 disabled:opacity-50"
      >
        {restarting ? "Restarting…" : "Restart Daemon"}
      </button>
    </div>
  );
}
