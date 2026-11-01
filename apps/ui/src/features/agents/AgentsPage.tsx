import { useEffect, useState } from "react";
import type {
  AgentDescriptor,
  TaskQueueEntry,
  TaskTreeNode,
} from "@talos/contracts";
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
  cancelSubtree,
  delegateTask,
  getPlanDocument,
  getTaskTree,
  isBackendUnavailable,
  isTauri,
  listTasks,
  setTaskMode,
  submitTask,
} from "../../bridge/index.js";
import { useApprovalStore } from "../../stores/approvalStore.js";
import {
  PlanView,
  getMockPlanDocument,
  useApprovalActions,
} from "../approval/index.js";
import { TaskQueueTable } from "./TaskQueueTable.js";
import { TaskForest, TaskTree } from "./TaskTree.js";

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

const SPECIALIST_ROLES = ["coder", "browser", "researcher"] as const;

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
  const [delegateParent, setDelegateParent] = useState("");
  const [delegateGoal, setDelegateGoal] = useState(
    "fix the code and research the docs",
  );
  const [delegateRole, setDelegateRole] = useState<
    (typeof SPECIALIST_ROLES)[number] | "auto"
  >("auto");
  const [delegating, setDelegating] = useState(false);
  const [tree, setTree] = useState<TaskTreeNode | null>(null);
  const [treeLoading, setTreeLoading] = useState(false);
  const pending = useApprovalStore((s) => s.pending);
  const openSheet = useApprovalStore((s) => s.openSheet);
  const waitingTasks = liveTasks.filter((t) => t.status === "waiting_policy");
  const [modeByTask, setModeByTask] = useState<
    Record<string, "plan" | "act" | "auto">
  >({});
  const [planByTask, setPlanByTask] = useState<
    Record<string, import("@talos/contracts").PlanDocument>
  >({});
  const { batchApprove } = useApprovalActions();

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

  const handleDelegate = async () => {
    const parentId =
      delegateParent.trim() !== "" ? delegateParent.trim() : liveTasks[0]?.id;
    if (!parentId) {
      setNotice("Submit a task first, then delegate it.");
      return;
    }
    setDelegating(true);
    setNotice(null);
    try {
      const input: {
        parentTaskId: string;
        goal?: string;
        role?: "coder" | "browser" | "researcher";
      } = {
        parentTaskId: parentId,
      };
      if (delegateRole === "auto") {
        input.goal = delegateGoal;
      } else {
        input.role = delegateRole;
      }
      const out = await delegateTask(input);
      upsertLiveTask(out.parent);
      for (const child of out.children) {
        upsertLiveTask(child);
      }
      appendFeed({
        id: `delegate-${parentId}-${Date.now()}`,
        at: new Date().toISOString(),
        actor: "supervisor",
        message: `Delegated ${parentId} -> ${out.children.length} children (${out.mergedStatus})`,
        tone: out.mergedStatus === "completed" ? "success" : "error",
        taskId: parentId,
      });
      const loaded = await getTaskTree(parentId).catch(() => null);
      if (loaded) {
        setTree(loaded);
      }
    } catch (error) {
      setNotice(error instanceof Error ? error.message : "Delegate failed");
    } finally {
      setDelegating(false);
    }
  };

  const handleLoadTree = async (taskId: string) => {
    setTreeLoading(true);
    try {
      const loaded = await getTaskTree(taskId);
      setTree(loaded);
    } catch (error) {
      setNotice(error instanceof Error ? error.message : "Load tree failed");
    } finally {
      setTreeLoading(false);
    }
  };

  const handleCancelSubtree = async (taskId: string) => {
    try {
      const updated = await cancelSubtree(taskId);
      upsertLiveTask(updated);
      appendFeed({
        id: `cancel-subtree-${taskId}-${Date.now()}`,
        at: new Date().toISOString(),
        actor: "supervisor",
        message: `Cancel subtree ${taskId} -> ${updated.status}`,
        tone: "info",
        taskId,
      });
    } catch (error) {
      setNotice(
        error instanceof Error ? error.message : "Cancel subtree failed",
      );
    }
  };

  const handleLoadPlan = async (taskId: string) => {
    if (!isTauri()) {
      setPlanByTask((prev) => ({
        ...prev,
        [taskId]: getMockPlanDocument(taskId),
      }));
      return;
    }
    try {
      const doc = await getPlanDocument(taskId);
      setPlanByTask((prev) => ({ ...prev, [taskId]: doc }));
    } catch (error) {
      setNotice(error instanceof Error ? error.message : "Load plan failed");
    }
  };

  const handleModeChange = async (
    taskId: string,
    mode: "plan" | "act" | "auto",
  ) => {
    setModeByTask((prev) => ({ ...prev, [taskId]: mode }));
    if (!isTauri()) return;
    try {
      await setTaskMode(taskId, mode);
    } catch (error) {
      setNotice(error instanceof Error ? error.message : "Set mode failed");
    }
  };

  const subAgents = liveTasks.filter(
    (task) => task.parentTaskId !== null && task.parentTaskId !== undefined,
  );

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
      <GlassPanel label="Active sub-agents">
        <SectionHeader
          title="Delegated specialists"
          subtitle={`${subAgents.length} active sub-agents · scoped tools · bounded context`}
        />
        {subAgents.length === 0 ? (
          <p className="text-xs text-slate-500">
            No delegated children yet — delegate a parent task to spawn coder /
            browser / researcher specialists.
          </p>
        ) : (
          <DataTable<TaskQueueEntry>
            caption="Sub-agents"
            rows={subAgents}
            keyOf={(row) => row.id}
            onRowSelect={(row) => {
              selectEntity({
                kind: "task",
                id: row.id,
                label: row.title,
                detail: `child of ${row.parentTaskId} · ${row.status}`,
              });
            }}
            columns={[
              { header: "Sub-agent task", render: (row) => row.title },
              {
                header: "Parent",
                render: (row) => (
                  <span className="font-mono text-xs">
                    {row.parentTaskId ?? "—"}
                  </span>
                ),
              },
              {
                header: "Status",
                render: (row) => (
                  <StatusPill tone={row.status === "completed" ? "ok" : "info"}>
                    {row.status}
                  </StatusPill>
                ),
              },
            ]}
          />
        )}
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
      {waitingTasks.length > 0 ? (
        <GlassPanel label="Waiting for approval">
          <SectionHeader
            title="Waiting for approval"
            subtitle={`${waitingTasks.length} tasks held at the policy gate · click to review`}
          />
          <ul className="flex flex-col gap-1">
            {waitingTasks.map((task) => {
              const count = pending.filter((a) => a.taskId === task.id).length;
              return (
                <li key={task.id}>
                  <button
                    type="button"
                    onClick={() => {
                      const first = pending.find((a) => a.taskId === task.id);
                      openSheet(first?.approvalId);
                      selectEntity({
                        kind: "task",
                        id: task.id,
                        label: task.title,
                        detail: `waiting_policy · ${count} approvals`,
                      });
                    }}
                    aria-label={`Review approvals for ${task.title}`}
                    className="approval-pulse flex w-full items-center gap-2 rounded-md border border-amber-400/30 bg-amber-400/10 px-2 py-1.5 text-left text-xs text-amber-200 motion-reduce:animate-none"
                  >
                    <span className="font-mono">{task.id}</span>
                    <span className="flex-1 truncate">{task.title}</span>
                    <StatusPill tone="warn">waiting_policy</StatusPill>
                  </button>
                </li>
              );
            })}
          </ul>
        </GlassPanel>
      ) : null}
      <GlassPanel label="Plan and act mode">
        <SectionHeader
          title="Plan / Act / Auto"
          subtitle="Plan proposes without executing · Act runs approved steps · Auto fast-paths low-risk reads"
        />
        <div className="mb-2 flex flex-wrap gap-2">
          {liveTasks.slice(0, 3).map((task) => (
            <button
              key={task.id}
              type="button"
              onClick={() => {
                void handleLoadPlan(task.id);
              }}
              className="rounded-md border border-white/10 px-2 py-1 font-mono text-xs text-cyan-300 hover:bg-white/5"
              aria-label={`Load plan for ${task.id}`}
            >
              Load plan {task.id.slice(0, 18)}
            </button>
          ))}
        </div>
        {Object.entries(planByTask).map(([taskId, doc]) => (
          <div
            key={taskId}
            className="mb-3 rounded-md border border-white/10 p-2"
          >
            <p className="mb-1 font-mono text-xs text-slate-400">{taskId}</p>
            <PlanView
              taskId={taskId}
              mode={modeByTask[taskId] ?? doc.mode}
              plan={doc}
              onModeChange={(mode) => {
                void handleModeChange(taskId, mode);
              }}
              onApproveOne={(approvalId) => {
                const found = pending.find((a) => a.approvalId === approvalId);
                if (found) openSheet(found.approvalId);
              }}
              onApproveLowRisk={() => {
                void batchApprove(taskId);
              }}
              busy={false}
            />
          </div>
        ))}
        {Object.keys(planByTask).length === 0 ? (
          <p className="text-xs text-slate-500">
            Load a plan to review ordered steps, switch modes, and batch-approve
            low-risk items.
          </p>
        ) : null}
      </GlassPanel>
      <GlassPanel label="Multi-agent delegation">
        <SectionHeader
          title="Supervisor delegation"
          subtitle="Decompose a parent into coder / browser / researcher slices"
          action={
            <NeonButton
              onClick={() => {
                void handleDelegate();
              }}
              loading={delegating}
            >
              Delegate task
            </NeonButton>
          }
        />
        <div className="mb-3 flex flex-wrap gap-2">
          <input
            aria-label="Parent task id"
            placeholder={liveTasks[0]?.id ?? "parent task id"}
            value={delegateParent}
            onChange={(event) => {
              setDelegateParent(event.target.value);
            }}
            className="min-w-52 flex-1 rounded-md border border-white/10 bg-black/30 px-2 py-1 font-mono text-xs"
          />
          <input
            aria-label="Delegation goal"
            value={delegateGoal}
            onChange={(event) => {
              setDelegateGoal(event.target.value);
            }}
            className="min-w-52 flex-1 rounded-md border border-white/10 bg-black/30 px-2 py-1 text-sm"
          />
          <select
            aria-label="Specialist role"
            value={delegateRole}
            onChange={(event) => {
              setDelegateRole(event.target.value as typeof delegateRole);
            }}
            className="rounded-md border border-white/10 bg-black/30 px-2 py-1 text-sm"
          >
            <option value="auto">auto (decompose)</option>
            {SPECIALIST_ROLES.map((r) => (
              <option key={r} value={r}>
                {r}
              </option>
            ))}
          </select>
        </div>
        <div className="mb-3 flex flex-wrap gap-2">
          {liveTasks.slice(0, 4).map((task) => (
            <span key={task.id} className="flex gap-1">
              <button
                type="button"
                onClick={() => {
                  void handleLoadTree(task.id);
                }}
                className="text-xs text-cyan-300 hover:underline"
              >
                Load tree
              </button>
              <button
                type="button"
                onClick={() => {
                  void handleCancelSubtree(task.id);
                }}
                className="text-xs text-amber-300 hover:underline"
              >
                Cancel subtree
              </button>
            </span>
          ))}
        </div>
        {treeLoading ? (
          <p className="text-xs text-slate-500">Loading tree…</p>
        ) : (
          <TaskTree tree={tree} />
        )}
        <div className="mt-3">
          <SectionHeader
            title="Task forest"
            subtitle="Roots → children from the live queue"
          />
          <TaskForest tasks={liveTasks} />
        </div>
      </GlassPanel>
    </div>
  );
}
