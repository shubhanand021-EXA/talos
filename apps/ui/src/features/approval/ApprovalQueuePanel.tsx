import type { ApprovalRequest } from "@talos/contracts";
import { useApprovalStore } from "../../stores/approvalStore.js";

/** Approval queue panel: ordered pendings, batch low-risk, click to review. */
export function ApprovalQueuePanel({
  onBatchApprove,
  busy,
}: {
  onBatchApprove: (taskId: string) => void;
  busy: boolean;
}) {
  const pending = useApprovalStore((s) => s.pending);
  const openSheet = useApprovalStore((s) => s.openSheet);
  if (pending.length === 0) {
    return (
      <p className="text-xs text-slate-500">
        No approvals waiting — the agent is clear to proceed.
      </p>
    );
  }
  const byTask = new Map<string, ApprovalRequest[]>();
  for (const a of pending) {
    const list = byTask.get(a.taskId) ?? [];
    list.push(a);
    byTask.set(a.taskId, list);
  }
  return (
    <div className="flex flex-col gap-2" role="list" aria-label="Pending approvals queue">
      {[...byTask.entries()].map(([taskId, items]) => (
        <div key={taskId} className="rounded-md border border-white/10 p-2">
          <div className="mb-1 flex items-center justify-between">
            <span className="font-mono text-xs text-slate-400">
              {taskId} · {items.length} waiting
            </span>
            <button
              type="button"
              onClick={() => {
                onBatchApprove(taskId);
              }}
              disabled={busy}
              aria-label={`Batch approve low-risk for ${taskId}`}
              className="rounded border border-emerald-400/30 px-2 py-0.5 text-xs text-emerald-200 hover:bg-emerald-400/10 disabled:opacity-50"
            >
              Approve low-risk
            </button>
          </div>
          <ul className="flex flex-col gap-1">
            {items.map((a) => (
              <li key={a.approvalId} role="listitem">
                <button
                  type="button"
                  onClick={() => {
                    openSheet(a.approvalId);
                  }}
                  aria-label={`Review ${a.summary}`}
                  className="flex w-full items-center gap-2 rounded bg-black/30 px-2 py-1.5 text-left text-xs hover:bg-white/5"
                >
                  <span
                    className={
                      a.riskLevel === "irreversible" || a.riskLevel === "high"
                        ? "h-2 w-2 shrink-0 rounded-full bg-rose-400"
                        : "h-2 w-2 shrink-0 rounded-full bg-amber-400"
                    }
                    aria-hidden="true"
                  />
                  <span className="min-w-0 flex-1 truncate text-slate-200">
                    {a.summary}
                  </span>
                  <span className="shrink-0 font-mono text-[11px] text-slate-500">
                    {a.effectClass}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        </div>
      ))}
    </div>
  );
}
