import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { isTauri } from "../../bridge/client.js";
import {
  parseDaemonEvent,
  parseScheduleNotification,
} from "../../bridge/gateway.js";

/** One desktop notification surfaced from a daemon broadcast. */
export interface DaemonNotification {
  id: string;
  kind: string;
  summary: string;
  taskId: string | null;
  at: string;
}

/**
 * Daemon broadcast subscriber. The Tauri shell re-emits every gateway
 * broadcast (scheduler firings, `PolicyRequired` desktop notifications) as
 * `gatewayDaemonEvent`; this hook collects them newest-first. Outside Tauri
 * (plain browser) it stays empty.
 */
export function useDaemonNotifications(): {
  notifications: DaemonNotification[];
  dismiss: (id: string) => void;
} {
  const [notifications, setNotifications] = useState<DaemonNotification[]>([]);

  useEffect(() => {
    if (!isTauri()) {
      return;
    }
    let cancelled = false;
    let unlisten: (() => void) | null = null;
    const attach = async () => {
      try {
        const stop = await listen<unknown>(
          "gatewayDaemonEvent",
          (tauriEvent) => {
            if (cancelled) {
              return;
            }
            const frame = parseDaemonEvent(tauriEvent.payload);
            if (frame === null) {
              return;
            }
            const notice = toNotification(frame);
            if (notice !== null) {
              setNotifications((prev) =>
                [notice, ...prev.filter((item) => item.id !== notice.id)].slice(
                  0,
                  10,
                ),
              );
            }
          },
        );
        if (!cancelled) {
          unlisten = stop;
        } else {
          stop();
        }
      } catch {
        // A failed subscription must not take the settings page down.
      }
    };
    void attach();
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  const dismiss = useCallback((id: string) => {
    setNotifications((prev) => prev.filter((item) => item.id !== id));
  }, []);

  return { notifications, dismiss };
}

function toNotification(frame: {
  event: string;
  payload: unknown;
  taskId?: string | null | undefined;
  at: string;
}): DaemonNotification | null {
  if (frame.event === "PolicyRequired") {
    const parsed = parseScheduleNotification(frame.payload);
    if (parsed === null) {
      return null;
    }
    return {
      id: `policy-${parsed.approvalId}`,
      kind: "PolicyRequired",
      summary: parsed.summary,
      taskId: parsed.taskId,
      at: frame.at,
    };
  }
  if (frame.event === "TaskStarted" || frame.event === "TaskCompleted") {
    const payload = frame.payload as {
      taskId?: unknown;
      status?: unknown;
    } | null;
    const taskId =
      typeof payload?.taskId === "string"
        ? payload.taskId
        : (frame.taskId ?? "unknown");
    const detail =
      frame.event === "TaskStarted"
        ? "started"
        : `→ ${typeof payload?.status === "string" ? payload.status : "done"}`;
    return {
      id: `${frame.event}-${taskId}-${frame.at}`,
      kind: frame.event,
      summary: `Scheduled task ${taskId} ${detail}`,
      taskId: frame.taskId ?? null,
      at: frame.at,
    };
  }
  return null;
}
