import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { KeyValueList, SectionHeader } from "../../design-system/index.js";
import { useUiStore } from "../../stores/uiStore.js";
import { useAppStore } from "../../stores/appStore.js";
import {
  cancelSubtree,
  cancelTask,
  getTaskTree,
  isBackendUnavailable,
  retryTask,
} from "../../bridge/index.js";

export function RightInspector() {
  const navigate = useNavigate();
  const selected = useUiStore((state) => state.selectedEntity);
  const selectEntity = useUiStore((state) => state.selectEntity);
  const liveTasks = useAppStore((state) => state.liveTasks);
  const upsertLiveTask = useAppStore((state) => state.upsertLiveTask);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const selectedTask =
    selected !== null && selected.kind === "task"
      ? (liveTasks.find((task) => task.id === selected.id) ?? null)
      : null;
  const taskResultMeta = useAppStore((state) => state.taskResultMeta);
  const selectedResultMeta =
    selectedTask !== null ? taskResultMeta[selectedTask.id] : undefined;

  const runAction = async (action: "cancel" | "retry") => {
    if (selectedTask === null) {
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const updated =
        action === "cancel"
          ? await cancelTask(selectedTask.id)
          : await retryTask(selectedTask.id);
      upsertLiveTask(updated);
    } catch (err) {
      if (isBackendUnavailable(err)) {
        setError("Backend unavailable — launch the desktop shell.");
      } else {
        setError(err instanceof Error ? err.message : "Action failed");
      }
    } finally {
      setBusy(false);
    }
  };

  const runSubtreeAction = async (action: "tree" | "cancel-subtree") => {
    if (selectedTask === null) {
      return;
    }
    setBusy(true);
    setError(null);
    try {
      if (action === "tree") {
        const loaded = await getTaskTree(selectedTask.id);
        const childCount = loaded.children.length;
        selectEntity({
          kind: "task",
          id: selectedTask.id,
          label: selectedTask.title,
          detail: `delegation: ${childCount} direct children · ${selectedTask.status}`,
        });
      } else {
        const updated = await cancelSubtree(selectedTask.id);
        upsertLiveTask(updated);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : "Action failed");
    } finally {
      setBusy(false);
    }
  };

  return (
    <aside
      aria-label="Inspector"
      className="hidden w-72 shrink-0 flex-col gap-4 overflow-y-auto border-l border-white/10 bg-void-900/60 p-4 xl:flex"
    >
      <SectionHeader title="Inspector" subtitle="Contextual details" />
      {selected === null ? (
        <div className="glass-panel p-4 text-sm text-slate-400">
          <p className="font-medium text-slate-200">Nothing selected</p>
          <p className="mt-1">
            Select an agent, task, or evidence row to inspect it here.
          </p>
        </div>
      ) : (
        <div className="glass-panel p-4">
          <p className="text-xs uppercase tracking-wider text-slate-500">
            {selected.kind}
          </p>
          <h3 className="mt-1 font-display text-sm font-semibold">
            {selected.label}
          </h3>
          <p className="mt-1 text-sm text-slate-400">{selected.detail}</p>
          <div className="mt-3">
            <KeyValueList entries={[{ key: "id", value: selected.id }]} />
          </div>
          {selected.kind === "tool" ? (
            <div className="mt-3 flex flex-col gap-2 border-t border-white/10 pt-3">
              <p className="text-xs text-slate-400">
                Ranked by the local Tool-RAG index. Execution still goes through
                policy-gated invocation.
              </p>
              <div className="flex gap-3">
                <button
                  type="button"
                  onClick={() => {
                    void navigate("/tools");
                  }}
                  className="text-xs text-cyan-300 hover:underline"
                >
                  View manifest
                </button>
              </div>
            </div>
          ) : null}
          {selected.kind === "evidence" ? (
            <div className="mt-3 flex flex-col gap-2 border-t border-white/10 pt-3">
              <p className="text-xs text-slate-400">
                Hash-chained ledger record. Open the Evidence page to verify the
                chain.
              </p>
              <div className="flex gap-3">
                <button
                  type="button"
                  onClick={() => {
                    void navigate("/evidence");
                  }}
                  className="text-xs text-cyan-300 hover:underline"
                >
                  View evidence
                </button>
              </div>
            </div>
          ) : null}
          {selectedTask !== null ? (
            <div className="mt-3 flex flex-col gap-2 border-t border-white/10 pt-3">
              <KeyValueList
                entries={[
                  { key: "status", value: selectedTask.status },
                  { key: "priority", value: selectedTask.priority },
                  {
                    key: "fidelity",
                    value: selectedTask.executionFidelity,
                  },
                  {
                    key: "attempts",
                    value: `${selectedTask.attempts}/${selectedTask.maxAttempts}`,
                  },
                  { key: "scenario", value: selectedTask.scenario },
                  {
                    key: "parent",
                    value:
                      selectedTask.parentTaskId ?? "root (supervisor scope)",
                  },
                  {
                    key: "updated",
                    value: selectedTask.updatedAt,
                  },
                ]}
              />
              {selectedResultMeta !== undefined &&
              selectedResultMeta.limitations.length > 0 ? (
                <div className="mt-2 rounded-md border border-white/10 bg-white/[0.02] p-2">
                  <p className="text-xs font-medium text-slate-300">
                    Limitations ({selectedResultMeta.fidelity})
                  </p>
                  <ul className="mt-1 list-disc pl-4 text-xs text-slate-400">
                    {selectedResultMeta.limitations.map((limitation) => (
                      <li key={limitation}>{limitation}</li>
                    ))}
                  </ul>
                </div>
              ) : null}
              {selectedTask.lastError !== null &&
              selectedTask.lastError !== undefined ? (
                <p className="text-xs text-rose-300">
                  {selectedTask.lastError}
                </p>
              ) : null}
              {error !== null ? (
                <p role="alert" className="text-xs text-red-300">
                  {error}
                </p>
              ) : null}
              <div className="flex gap-3">
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => {
                    void runAction("cancel");
                  }}
                  className="text-xs text-amber-300 hover:underline disabled:opacity-50"
                >
                  Cancel task
                </button>
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => {
                    void runAction("retry");
                  }}
                  className="text-xs text-cyan-300 hover:underline disabled:opacity-50"
                >
                  Retry task
                </button>
              </div>
              <div className="flex gap-3">
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => {
                    void runSubtreeAction("tree");
                  }}
                  className="text-xs text-cyan-300 hover:underline disabled:opacity-50"
                >
                  Show delegation
                </button>
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => {
                    void runSubtreeAction("cancel-subtree");
                  }}
                  className="text-xs text-amber-300 hover:underline disabled:opacity-50"
                >
                  Cancel subtree
                </button>
              </div>
              <div className="flex gap-3">
                <button
                  type="button"
                  onClick={() => {
                    void navigate("/tools");
                  }}
                  className="text-xs text-cyan-300 hover:underline"
                >
                  View receipt
                </button>
                <button
                  type="button"
                  onClick={() => {
                    void navigate("/evidence");
                  }}
                  className="text-xs text-cyan-300 hover:underline"
                >
                  View evidence
                </button>
              </div>
            </div>
          ) : null}
          <button
            type="button"
            onClick={() => {
              selectEntity(null);
            }}
            className="mt-3 text-xs text-cyan-300 hover:underline"
          >
            Clear selection
          </button>
        </div>
      )}
      <div className="glass-panel p-4">
        <h3 className="text-xs font-semibold uppercase tracking-wider text-slate-400">
          Live loop
        </h3>
        <p className="mt-1 text-sm text-slate-400">
          Policy evaluations stream here from the local runtime. Run a
          diagnostic from the Dashboard, then select any node or row for
          details.
        </p>
      </div>
    </aside>
  );
}
