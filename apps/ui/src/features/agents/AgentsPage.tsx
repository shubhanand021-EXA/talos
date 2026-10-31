import { useEffect, useState } from "react";
import type { AgentDescriptor, TaskQueueEntry } from "@talos/contracts";
import {
  DataTable,
  GlassPanel,
  NeonButton,
  SectionHeader,
  StatusPill,
  type PillTone,
} from "../../design-system/index.js";
import { getMockAgents } from "../../stores/mockStore.js";
import { useAppStore } from "../../stores/appStore.js";
import { useUiStore } from "../../stores/uiStore.js";
import {
  isBackendUnavailable,
  listTasks,
  submitTask,
} from "../../bridge/index.js";
import { TaskQueueTable } from "./TaskQueueTable.js";

const STATUS_TONE: Record<AgentDescriptor["status"], PillTone> = {
  idle: "neutral",
  starting: "info",
  running: "ok",
  waiting_policy: "warn",
  failed: "error",
  terminated: "neutral",
  completed: "ok",
};

const SCENARIOS = [
  "quick_success",
  "delayed_success",
  "retryable_failure",
  "non_retryable_failure",
  "cancellation_sensitive",
] as const;

export function AgentsPage() {
  const agents = getMockAgents();
  const selectEntity = useUiStore((state) => state.selectEntity);
  const liveTasks = useAppStore((state) => state.liveTasks);
  const setLiveTasks = useAppStore((state) => state.setLiveTasks);
  const upsertLiveTask = useAppStore((state) => state.upsertLiveTask);
  const appendFeed = useAppStore((state) => state.appendFeed);
  const [title, setTitle] = useState("Triage inbox batch");
  const [priority, setPriority] = useState("normal");
  const [scenario, setScenario] =
    useState<(typeof SCENARIOS)[number]>("quick_success");
  const [submitting, setSubmitting] = useState(false);
  const [loading, setLoading] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    const refresh = async () => {
      setLoading(true);
      try {
        const tasks = await listTasks(50);
        if (!cancelled) {
          setLiveTasks(tasks);
        }
      } catch {
        if (!cancelled) {
          setNotice("Backend unavailable — showing live queue only.");
        }
      } finally {
        if (!cancelled) {
          setLoading(false);
        }
      }
    };
    void refresh();
    return () => {
      cancelled = true;
    };
  }, [setLiveTasks]);

  const handleSubmit = async () => {
    setSubmitting(true);
    setNotice(null);
    try {
      const task = await submitTask({
        title,
        priority: priority as TaskQueueEntry["priority"],
        scenario,
      });
      upsertLiveTask(task);
      appendFeed({
        id: `submit-${task.id}-${Date.now()}`,
        at: new Date().toISOString(),
        actor: "runtime",
        message: `Submitted ${task.title} -> ${task.status}`,
        tone: "success",
        taskId: task.id,
      });
    } catch (error) {
      if (isBackendUnavailable(error)) {
        setNotice("Backend unavailable — launch the desktop shell.");
      } else {
        setNotice(error instanceof Error ? error.message : "Submit failed");
      }
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div className="flex flex-col gap-4">
      <SectionHeader
        title="Agents"
        subtitle={`${agents.length} configured · deterministic mock roster`}
      />
      <GlassPanel label="Agent roster">
        <DataTable<AgentDescriptor>
          caption="Agent roster"
          rows={agents}
          keyOf={(row) => row.id}
          onRowSelect={(row) => {
            selectEntity({
              kind: "agent",
              id: row.id,
              label: row.displayName,
              detail: `${row.role} · ${row.status} · ${row.capabilities.join(", ")}`,
            });
          }}
          columns={[
            { header: "Agent", render: (row) => row.displayName },
            {
              header: "Role",
              render: (row) => (
                <span className="font-mono text-xs">{row.role}</span>
              ),
            },
            {
              header: "Status",
              render: (row) => (
                <StatusPill tone={STATUS_TONE[row.status]}>
                  {row.status}
                </StatusPill>
              ),
            },
            {
              header: "Capabilities",
              render: (row) => row.capabilities.join(", "),
            },
          ]}
        />
      </GlassPanel>
      <GlassPanel label="Task queue">
        <SectionHeader
          title="Durable task queue"
          subtitle={
            loading
              ? "Refreshing from backend…"
              : `${liveTasks.length} live tasks · priority ordered · SQLite backed`
          }
          action={
            <NeonButton
              onClick={() => {
                void handleSubmit();
              }}
              loading={submitting}
            >
              Submit task
            </NeonButton>
          }
        />
        <div className="mb-3 flex flex-wrap gap-2">
          <input
            aria-label="Task title"
            value={title}
            onChange={(event) => {
              setTitle(event.target.value);
            }}
            className="min-w-52 flex-1 rounded-md border border-white/10 bg-black/30 px-2 py-1 text-sm"
          />
          <select
            aria-label="Priority"
            value={priority}
            onChange={(event) => {
              setPriority(event.target.value);
            }}
            className="rounded-md border border-white/10 bg-black/30 px-2 py-1 text-sm"
          >
            {["low", "normal", "high", "critical"].map((p) => (
              <option key={p} value={p}>
                {p}
              </option>
            ))}
          </select>
          <select
            aria-label="Scenario"
            value={scenario}
            onChange={(event) => {
              setScenario(event.target.value as typeof scenario);
            }}
            className="rounded-md border border-white/10 bg-black/30 px-2 py-1 text-sm"
          >
            {SCENARIOS.map((s) => (
              <option key={s} value={s}>
                {s}
              </option>
            ))}
          </select>
        </div>
        {notice !== null ? (
          <p role="status" className="mb-2 text-xs text-amber-300">
            {notice}
          </p>
        ) : null}
        <TaskQueueTable tasks={liveTasks} />
      </GlassPanel>
    </div>
  );
}
