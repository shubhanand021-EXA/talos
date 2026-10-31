import { useState } from "react";
import type { TaskQueueEntry } from "@talos/contracts";
import {
  DataTable,
  StatusPill,
  type PillTone,
} from "../../design-system/index.js";
import {
  cancelTask,
  isBackendUnavailable,
  retryTask,
} from "../../bridge/index.js";
import { useAppStore } from "../../stores/appStore.js";
import { useUiStore } from "../../stores/uiStore.js";

const STATUS_TONE: Record<string, PillTone> = {
  queued: "neutral",
  planning: "info",
  executing: "info",
  waiting_policy: "warn",
  waiting_evidence: "warn",
  waiting_compensation: "warn",
  canceling: "warn",
  canceled: "neutral",
  completed: "ok",
  failed: "error",
  timed_out: "error",
};

const PRIORITY_TONE: Record<string, PillTone> = {
  low: "neutral",
  normal: "info",
  high: "warn",
  critical: "error",
};

export function TaskQueueTable({ tasks }: { tasks: TaskQueueEntry[] }) {
  const selectEntity = useUiStore((state) => state.selectEntity);
  const upsertLiveTask = useAppStore((state) => state.upsertLiveTask);
  const appendFeed = useAppStore((state) => state.appendFeed);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const runAction = async (id: string, action: "cancel" | "retry") => {
    setBusyId(id);
    setError(null);
    try {
      const updated =
        action === "cancel" ? await cancelTask(id) : await retryTask(id);
      upsertLiveTask(updated);
      appendFeed({
        id: `queue-${action}-${id}-${Date.now()}`,
        at: new Date().toISOString(),
        actor: "runtime",
        message: `Task ${id} ${action} -> ${updated.status}`,
        tone: "info",
        taskId: id,
      });
    } catch (err) {
      const message =
        err instanceof Error ? err.message : `Task ${action} failed`;
      if (isBackendUnavailable(err)) {
        setError("Backend unavailable — launch the desktop shell.");
      } else {
        setError(message);
      }
    } finally {
      setBusyId(null);
    }
  };

  return (
    <div className="flex flex-col gap-2">
      {error !== null ? (
        <p role="alert" className="text-xs text-red-300">
          {error}
        </p>
      ) : null}
      <DataTable<TaskQueueEntry>
        caption="Task queue"
        rows={tasks}
        keyOf={(row) => row.id}
        onRowSelect={(row) => {
          selectEntity({
            kind: "task",
            id: row.id,
            label: row.title,
            detail: `${row.status} · ${row.priority} · attempt ${row.attempts}/${row.maxAttempts}`,
          });
        }}
        columns={[
          { header: "Task", render: (row) => row.title },
          {
            header: "Status",
            render: (row) => (
              <StatusPill tone={STATUS_TONE[row.status] ?? "neutral"}>
                {row.status}
              </StatusPill>
            ),
          },
          {
            header: "Priority",
            render: (row) => (
              <StatusPill tone={PRIORITY_TONE[row.priority] ?? "neutral"}>
                {row.priority}
              </StatusPill>
            ),
          },
          {
            header: "Attempts",
            render: (row) => (
              <span className="font-mono text-xs">
                {row.attempts}/{row.maxAttempts}
              </span>
            ),
          },
          {
            header: "Actions",
            render: (row) => (
              <span className="flex gap-2">
                <button
                  type="button"
                  disabled={busyId === row.id}
                  onClick={(event) => {
                    event.stopPropagation();
                    void runAction(row.id, "cancel");
                  }}
                  className="text-xs text-amber-300 hover:underline disabled:opacity-50"
                >
                  Cancel
                </button>
                <button
                  type="button"
                  disabled={busyId === row.id}
                  onClick={(event) => {
                    event.stopPropagation();
                    void runAction(row.id, "retry");
                  }}
                  className="text-xs text-cyan-300 hover:underline disabled:opacity-50"
                >
                  Retry
                </button>
              </span>
            ),
          },
        ]}
      />
    </div>
  );
}
