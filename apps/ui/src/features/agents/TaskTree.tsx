/* eslint-disable react-refresh/only-export-components -- forest helper lives with its view */
import type { TaskQueueEntry, TaskTreeNode } from "@talos/contracts";
import { StatusPill, type PillTone } from "../../design-system/index.js";
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

function parentOf(task: TaskQueueEntry): string | null {
  const raw = (task as unknown as { parentTaskId?: unknown }).parentTaskId;
  return typeof raw === "string" ? raw : null;
}

function TaskRow({ task, depth }: { task: TaskQueueEntry; depth: number }) {
  const selectEntity = useUiStore((state) => state.selectEntity);
  const parent = parentOf(task);
  return (
    <button
      type="button"
      onClick={() => {
        selectEntity({
          kind: "task",
          id: task.id,
          label: task.title,
          detail: `${task.status} · ${task.priority} · attempt ${task.attempts}/${task.maxAttempts}${parent ? ` · child of ${parent}` : ""}`,
        });
      }}
      className="flex w-full items-center gap-2 rounded px-2 py-1 text-left hover:bg-white/5"
      style={{ paddingLeft: `${depth * 1.25 + 0.5}rem` }}
    >
      <span aria-hidden="true" className="font-mono text-xs text-slate-500">
        {depth === 0 ? "◇" : "↳"}
      </span>
      <span className="min-w-0 flex-1 truncate text-sm text-slate-200">
        {task.title}
      </span>
      <StatusPill tone={STATUS_TONE[task.status] ?? "neutral"}>
        {task.status}
      </StatusPill>
      {parent !== null ? (
        <span className="font-mono text-[10px] text-slate-500">child</span>
      ) : null}
    </button>
  );
}

function TreeNode({ node, depth }: { node: TaskTreeNode; depth: number }) {
  return (
    <div>
      <TaskRow task={node.task} depth={depth} />
      {node.children.map((child: TaskTreeNode) => (
        <TreeNode key={child.task.id} node={child} depth={depth + 1} />
      ))}
    </div>
  );
}

/** Parent-child task hierarchy (Batch 3 delegation tree). */
export function TaskTree({ tree }: { tree: TaskTreeNode | null }) {
  if (tree === null) {
    return (
      <p className="text-xs text-slate-500">
        Select a task and load its tree to see parent → child delegation edges.
      </p>
    );
  }
  return (
    <div
      aria-label="Task tree"
      className="flex max-h-64 flex-col gap-0.5 overflow-y-auto rounded-md border border-white/10 bg-black/30 p-2"
    >
      <TreeNode node={tree} depth={0} />
    </div>
  );
}

/** Build a forest from a flat queue list (roots first, children nested). */
export function forestFromQueue(tasks: TaskQueueEntry[]): TaskTreeNode[] {
  const byId = new Map<string, TaskTreeNode>();
  for (const task of tasks) {
    byId.set(task.id, { task, children: [] });
  }
  const roots: TaskTreeNode[] = [];
  for (const node of byId.values()) {
    const parent = parentOf(node.task);
    if (parent !== null && byId.has(parent)) {
      byId.get(parent)?.children.push(node);
    } else {
      roots.push(node);
    }
  }
  return roots;
}

/** Flat forest view (no backend round-trip) for the Agents page. */
export function TaskForest({ tasks }: { tasks: TaskQueueEntry[] }) {
  const roots = forestFromQueue(tasks);
  if (roots.length === 0) {
    return (
      <p className="text-xs text-slate-500">
        No live tasks yet — delegation trees appear here.
      </p>
    );
  }
  return (
    <div aria-label="Task forest" className="flex flex-col gap-0.5">
      {roots.map((root) => (
        <TreeNode key={root.task.id} node={root} depth={0} />
      ))}
    </div>
  );
}
