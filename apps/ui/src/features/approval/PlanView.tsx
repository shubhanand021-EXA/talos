import type { PlanDocument, TaskMode } from "@talos/contracts";

const MODES: Array<{ mode: TaskMode; hint: string }> = [
  { mode: "plan", hint: "Propose only — nothing executes until approved" },
  { mode: "act", hint: "Execute already-approved actions" },
  { mode: "auto", hint: "Auto-approve low-risk reads, gate the rest" },
];

/**
 * Plan/Act separation surface: mode toggle per task plus the ordered
 * review list. Approve-all is low-risk only; high-risk stays individual.
 */
export function PlanView({
  taskId,
  mode,
  plan,
  onModeChange,
  onApproveOne,
  onApproveLowRisk,
  busy,
}: {
  taskId: string;
  mode: TaskMode;
  plan: PlanDocument | null;
  onModeChange: (mode: TaskMode) => void;
  onApproveOne: (approvalId: string) => void;
  onApproveLowRisk: () => void;
  busy: boolean;
}) {
  return (
    <section aria-label={`Plan and act mode for ${taskId}`}>
      <div
        role="radiogroup"
        aria-label="Task mode: plan, act, or auto"
        className="flex gap-1 rounded-md border border-white/10 bg-black/30 p-1"
      >
        {MODES.map(({ mode: m, hint }) => (
          <button
            key={m}
            type="button"
            role="radio"
            aria-checked={mode === m}
            title={hint}
            onClick={() => {
              onModeChange(m);
            }}
            className={
              mode === m
                ? "flex-1 rounded bg-cyan-400/20 px-3 py-1.5 text-sm font-semibold text-cyan-200"
                : "flex-1 rounded px-3 py-1.5 text-sm text-slate-400 hover:bg-white/5"
            }
          >
            {m === "plan" ? "Plan" : m === "act" ? "Act" : "Auto"}
          </button>
        ))}
      </div>
      <p className="mt-1 text-xs text-slate-500">
        {MODES.find((m) => m.mode === mode)?.hint} · Default is Plan (safest).
      </p>

      {plan === null ? (
        <p className="mt-2 text-xs text-slate-500">
          No plan yet for {taskId} — proposed actions will appear here in order.
        </p>
      ) : (
        <div className="mt-2">
          <div className="mb-2 flex items-center justify-between">
            <p className="font-mono text-xs text-slate-400">
              {plan.approvedCount} approved · {plan.pendingCount} pending ·{" "}
              {plan.actions.length} total
            </p>
            <button
              type="button"
              onClick={onApproveLowRisk}
              disabled={busy || plan.pendingCount === 0}
              aria-label="Batch approve low-risk items"
              className="rounded-md border border-emerald-400/30 bg-emerald-400/10 px-2 py-1 text-xs text-emerald-200 hover:bg-emerald-400/20 disabled:opacity-50"
            >
              Approve low-risk
            </button>
          </div>
          <ol className="flex flex-col gap-1">
            {plan.actions.map((action, i) => (
              <li
                key={action.approvalId}
                className="flex items-center gap-2 rounded-md border border-white/10 bg-black/30 p-2 text-xs"
              >
                <span className="font-mono text-slate-500">{i + 1}.</span>
                <div className="min-w-0 flex-1">
                  <p className="truncate text-slate-200">
                    {action.description}
                  </p>
                  <p className="font-mono text-[11px] text-slate-500">
                    {action.target} · {action.effect} · {action.risk} ·{" "}
                    {action.status}
                  </p>
                </div>
                {action.status === "pending" ? (
                  <button
                    type="button"
                    onClick={() => {
                      onApproveOne(action.approvalId);
                    }}
                    disabled={busy}
                    aria-label={`Approve step ${i + 1}`}
                    className="shrink-0 rounded border border-emerald-400/30 px-2 py-0.5 text-emerald-200 hover:bg-emerald-400/10 disabled:opacity-50"
                  >
                    Approve
                  </button>
                ) : (
                  <span className="shrink-0 font-mono text-[11px] text-slate-500">
                    {action.status}
                  </span>
                )}
              </li>
            ))}
          </ol>
        </div>
      )}
    </section>
  );
}
