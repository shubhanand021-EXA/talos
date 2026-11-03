import { useCallback, useEffect, useState } from "react";
import {
  GatewayClient,
  getGatewayStatus,
  restartGatewayDaemon,
  type GatewayStatusView,
} from "../../bridge/gateway.js";
import { isTauri } from "../../bridge/client.js";

const INITIAL: GatewayStatusView = {
  link: "unknown",
  url: "ws://127.0.0.1:8080/talos",
  missedPongs: 0,
  lastSequence: null,
  daemonVersion: null,
};

/**
 * Daemon link state. Native path: polls the Tauri `gateway_status` command
 * (the shell owns the socket + heartbeat). Browser path: runs a local
 * `GatewayClient` with heartbeat + exponential-backoff reconnect.
 */
export function useGatewayStatus(pollMs = 5_000): GatewayStatusView {
  const [status, setStatus] = useState<GatewayStatusView>(INITIAL);

  useEffect(() => {
    let cancelled = false;
    if (isTauri()) {
      const poll = async () => {
        try {
          const next = await getGatewayStatus();
          if (!cancelled) {
            setStatus(next);
          }
        } catch {
          if (!cancelled) {
            setStatus((prev) => ({ ...prev, link: "disconnected" }));
          }
        }
      };
      void poll();
      const timer = setInterval(() => {
        void poll();
      }, pollMs);
      return () => {
        cancelled = true;
        clearInterval(timer);
      };
    }
    const client = new GatewayClient({ onStatus: setStatus });
    client.connect();
    return () => {
      client.disconnect();
    };
  }, [pollMs]);

  return status;
}

/** Restart the daemon (powers the DISCONNECTED banner button). */
export function useRestartGateway(): {
  restarting: boolean;
  restart: () => Promise<void>;
} {
  const [restarting, setRestarting] = useState(false);
  const restart = useCallback(async () => {
    setRestarting(true);
    try {
      await restartGatewayDaemon();
    } catch {
      // The button stays available; the next poll reports the outcome.
    } finally {
      setRestarting(false);
    }
  }, []);
  return { restarting, restart };
}
