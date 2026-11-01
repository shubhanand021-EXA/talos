import { useState } from "react";
import type { CompensationExecution } from "@talos/contracts";

/**
 * Compensation sheet: what went wrong, what the rollback will do, live
 * per-step progress. Confirming runs the steps; a failed step escalates the
 * task to `failed_needs_manual_review` and alerts the reviewer.
 */
export function CompensationSheet({
  execution,
  onConfirm,
  onFailStep,
  busy,
}: {
  execution: CompensationExecution;
  onConfirm: (execution: CompensationExecution) => void;
  onFailStep: (execution: CompensationExecution) => void;
  busy: boolean;
}) {
  const [confirmed, setConfirmed] = useState(false);
  const done = execution.steps.filter((s) => s.status === "done").length;
  const total = execution.steps.length;
  const pct = total === 0 ? 100 : Math.round((done / total) * 100);
  const needsReview = execution.status === "failed_needs_manual_review";

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4"
      role="presentation"
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="comp-title"
        className="glass-panel max-h-[90vh] w-full max-w-xl overflow-y-auto p-5 ring-2 ring-amber-500/50"
      >
        <h2 id="comp-title" className="font-display text-lg font-semibold">
          Compensation required
        </h2>
        <p className="font-mono text-xs text-slate-500">
          {execution.planId} · task {execution.taskId}
        </p>
        <p role="alert" className="mt-2 text-sm text-amber-200">
          {execution.failureReason}
        </p>

        <div
          className="mt-3"
          role="progressbar"
          aria-valuenow={pct}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-label="Compensation progress"
        >
          <div className="h-2 overflow-hidden rounded bg-white/10">
            <div
              className={`h-full ${needsReview ? "bg-rose-500" : "bg-amber-400"}`}
              style={{ width: `${pct}%` }}
            />
          </div>
          <p className="mt-1 font-mono text-xs text-slate-400">
            {done}/{total} steps · {execution.status}
          </p>
        </div>

        <ol aria-label="Compensation steps" className="mt-3 flex flex-col gap-2">
          {execution.steps.map((step) => (
            <li
              key={step.order}
              className="flex items-center gap-2 rounded-md border border-white/10 bg-black/40 p-2 text-sm"
            >
              <span
                aria-label={`Step ${step.order + 1} ${step.status}`}
                className={
                  step.status === "done"
                    ? "font-mono text-emerald-300"
                    : step.status === "failed"
                      ? "font-mono text-rose-300"
                      : "font-mono text-slate-500"
                }
              >
                {step.status === "done"
                  ? "✓"
                  : step.status === "failed"
                    ? "✕"
                    : step.status === "running"
                      ? "…"
                      : "○"}
              </span>
              <div>
                <p className="font-mono text-xs text-slate-200">{step.actionType}</p>
                <p className="text-xs text-slate-400">{step.description}</p>
              </div>
              <span className="ml-auto font-mono text-[11px] text-slate-500">
                {step.status}
              </span>
            </li>
          ))}
        </ol>

        {needsReview ? (
          <p role="alert" className="mt-3 rounded-md border border-rose-400/40 bg-rose-500/10 p-2 text-sm text-rose-200">
            Compensation failed. Task marked{" "}
            <span className="font-mono">failed_needs_manual_review</span> — a
            human must finish the rollback by hand.
          </p>
        ) : null}

        <div className="mt-4 flex gap-2">
          {!confirmed ? (
            <button
              type="button"
              onClick={() => {
                setConfirmed(true);
                onConfirm(execution);
              }}
              disabled={busy}
              aria-label="Approve and run compensation"
              className="rounded-md bg-amber-400 px-4 py-2 text-sm font-semibold text-black hover:bg-amber-300 disabled:opacity-50"
            >
              Approve compensation
            </button>
          ) : (
            <button
              type="button"
              onClick={() => {
                onFailStep(execution);
              }}
              disabled={busy}
              aria-label="Simulate failing step (test hook)"
              className="rounded-md border border-white/10 px-3 py-2 text-xs text-slate-400 hover:bg-white/5"
            >
              Fail next step (test)
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
