import { useCallback, useEffect, useState } from "react";
import type { ScheduleStatusView } from "@talos/contracts";
import { isTauri } from "../../bridge/client.js";
import {
  listSchedules,
  toggleSchedule,
  triggerScheduleNow,
} from "../../bridge/gateway.js";
import {
  GlassPanel,
  NeonButton,
  SectionHeader,
  Spinner,
  StatusPill,
} from "../../design-system/index.js";
import { useDaemonNotifications } from "./useDaemonNotifications.js";

/**
 * Heartbeats panel (Settings): active schedules with next trigger, last
 * execution status, pause/resume + Trigger-now controls, and live desktop
 * notifications for scheduler firings parked in `waiting_policy`.
 */
export function HeartbeatsPanel() {
  const [schedules, setSchedules] = useState<ScheduleStatusView[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const { notifications, dismiss } = useDaemonNotifications();

  const refresh = useCallback(async () => {
    if (!isTauri()) {
      setError("Scheduler lives in the Tauri shell — launch the desktop app.");
      setSchedules([]);
      return;
    }
    try {
      setSchedules(await listSchedules());
      setError(null);
    } catch (cause) {
      setError(
        cause instanceof Error ? cause.message : "Scheduler unreachable",
      );
      setSchedules([]);
    }
  }, []);

  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      if (!isTauri()) {
        if (!cancelled) {
          setError(
            "Scheduler lives in the Tauri shell — launch the desktop app.",
          );
          setSchedules([]);
        }
        return;
      }
      try {
        const rows = await listSchedules();
        if (!cancelled) {
          setSchedules(rows);
          setError(null);
        }
      } catch (cause) {
        if (!cancelled) {
          setError(
            cause instanceof Error ? cause.message : "Scheduler unreachable",
          );
          setSchedules([]);
        }
      }
    };
    void load();
    return () => {
      cancelled = true;
    };
  }, []);

  const flip = useCallback(async (id: string, active: boolean) => {
    setBusy(id);
    try {
      const rows = await toggleSchedule(id, active);
      setSchedules((prev) => {
        if (prev === null) {
          return rows;
        }
        const byId = new Map(rows.map((row) => [row.id, row]));
        return prev.map((row) => byId.get(row.id) ?? row);
      });
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Toggle failed");
    } finally {
      setBusy(null);
    }
  }, []);

  const fireNow = useCallback(
    async (id: string) => {
      setBusy(id);
      try {
        await triggerScheduleNow(id);
        await refresh();
      } catch (cause) {
        setError(cause instanceof Error ? cause.message : "Trigger failed");
      } finally {
        setBusy(null);
      }
    },
    [refresh],
  );

  return (
    <GlassPanel label="Heartbeats">
      <SectionHeader
        title="Scheduled heartbeats"
        subtitle="HEARTBEAT.md schedules firing system-initiated Supervisor tasks (UTC, policy-gated)"
      />
      {notifications.length > 0 ? (
        <ul
          aria-label="Scheduler notifications"
          className="mb-3 flex flex-col gap-2"
        >
          {notifications.map((notice) => (
            <li
              key={notice.id}
              role={notice.kind === "PolicyRequired" ? "alert" : "status"}
              className={`flex items-center justify-between gap-3 rounded-md border px-3 py-2 text-sm ${
                notice.kind === "PolicyRequired"
                  ? "border-amber-400/40 bg-amber-400/10 text-amber-100"
                  : "border-cyan-300/30 bg-cyan-400/10 text-cyan-100"
              }`}
            >
              <span>
                <strong className="font-semibold">{notice.kind}</strong>
                <span className="ml-2 text-sm opacity-80">
                  {notice.summary}
                </span>
              </span>
              <button
                type="button"
                aria-label={`Dismiss ${notice.kind}`}
                onClick={() => {
                  dismiss(notice.id);
                }}
                className="shrink-0 rounded border border-white/20 px-2 py-0.5 text-xs hover:bg-white/10"
              >
                Dismiss
              </button>
            </li>
          ))}
        </ul>
      ) : null}
      {schedules === null ? (
        <Spinner label="Loading schedules" />
      ) : schedules.length === 0 ? (
        <p className="text-sm text-slate-400" role="status">
          {error ??
            "No schedules — add them to HEARTBEAT.md next to the app data dir."}
        </p>
      ) : (
        <ul className="flex flex-col gap-2">
          {schedules.map((schedule) => (
            <li
              key={schedule.id}
              className="flex flex-wrap items-center justify-between gap-3 rounded-md border border-white/10 px-3 py-2"
            >
              <div className="min-w-0">
                <div className="flex items-center gap-2">
                  <StatusPill tone={schedule.active ? "ok" : "neutral"}>
                    {schedule.active ? "active" : "paused"}
                  </StatusPill>
                  <span className="font-medium text-slate-100">
                    {schedule.name}
                  </span>
                  <span className="font-mono text-xs text-slate-400">
                    {schedule.cron} UTC
                  </span>
                </div>
                <p
                  className="mt-1 truncate text-xs text-slate-400"
                  title={schedule.expression}
                >
                  {schedule.expression}
                </p>
                <p className="mt-0.5 text-xs text-slate-500">
                  {`next ${schedule.nextTriggerAt} · last ${schedule.lastStatus ?? "never"}${schedule.lastRunAt ? ` @ ${schedule.lastRunAt}` : ""} · ${schedule.runCount} runs`}
                </p>
              </div>
              <div className="flex items-center gap-2">
                <NeonButton
                  variant="ghost"
                  disabled={busy === schedule.id}
                  onClick={() => {
                    void flip(schedule.id, !schedule.active);
                  }}
                >
                  {busy === schedule.id
                    ? "Working…"
                    : schedule.active
                      ? "Pause"
                      : "Resume"}
                </NeonButton>
                <NeonButton
                  variant="primary"
                  disabled={busy === schedule.id}
                  onClick={() => {
                    void fireNow(schedule.id);
                  }}
                >
                  Trigger now
                </NeonButton>
              </div>
            </li>
          ))}
        </ul>
      )}
      {error !== null && schedules !== null && schedules.length > 0 ? (
        <p className="mt-2 text-xs text-amber-300" role="status">
          {error}
        </p>
      ) : null}
    </GlassPanel>
  );
}
