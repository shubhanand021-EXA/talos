import type { PlanPreviewView } from "@talos/contracts";
import {
  GlassPanel,
  SectionHeader,
  StatusPill,
} from "../../design-system/index.js";

export type PlanViewerProps = {
  plan: PlanPreviewView;
  onApprove?: () => void;
  onReject?: () => void;
  busy?: boolean;
};

/** DAG plan viewer: steps in topological order, levels, validation status. */
export function PlanViewer({
  plan,
  onApprove,
  onReject,
  busy,
}: PlanViewerProps) {
  const levelOf = new Map<string, number>();
  plan.levels.forEach((level, index) => {
    for (const id of level) levelOf.set(id, index);
  });
  return (
    <GlassPanel label="DAG plan">
      <SectionHeader
        title={`Plan · ${plan.method}`}
        subtitle={`${plan.steps.length} steps · ${plan.levels.length} levels · ${plan.valid ? "valid" : "invalid"}`}
      />
      <div className="mb-2 flex flex-wrap items-center gap-2">
        <StatusPill tone={plan.method === "llm" ? "ok" : "neutral"}>
          {plan.method === "llm" ? "LLM" : "keyword fallback"}
        </StatusPill>
        <StatusPill tone={plan.valid ? "ok" : "error"}>
          {plan.valid ? "valid" : "rejected"}
        </StatusPill>
        {plan.requiresApproval ? (
          <StatusPill tone="warn">requires approval</StatusPill>
        ) : null}
      </div>
      {plan.fallbackReason ? (
        <p role="note" className="mb-2 text-xs text-amber-300">
          Fallback: {plan.fallbackReason}
        </p>
      ) : null}
      <ol className="flex flex-col gap-2">
        {plan.steps.map((step) => (
          <li
            key={step.stepId}
            data-testid={`plan-step-${step.stepId}`}
            className="rounded-md border border-white/10 p-2"
          >
            <div className="flex flex-wrap items-center gap-2">
              <span className="font-mono text-xs text-cyan-300">
                {step.stepId}
              </span>
              <StatusPill tone="info">{step.role}</StatusPill>
              <span className="text-xs text-slate-400">
                level {levelOf.get(step.stepId) ?? 0}
              </span>
              {step.requiresApproval ? (
                <StatusPill tone="warn">high-risk</StatusPill>
              ) : null}
              <span className="ml-auto font-mono text-xs text-slate-500">
                ~{step.estimatedTokens} tokens
              </span>
            </div>
            <p className="mt-1 text-sm">{step.goal}</p>
            {step.dependsOn.length > 0 ? (
              <p className="mt-1 font-mono text-xs text-slate-500">
                depends on: {step.dependsOn.join(", ")}
              </p>
            ) : null}
            {step.toolsNeeded.length > 0 ? (
              <p className="mt-1 font-mono text-xs text-slate-500">
                tools: {step.toolsNeeded.join(", ")}
              </p>
            ) : null}
          </li>
        ))}
      </ol>
      {plan.steps.length === 0 ? (
        <p className="text-xs text-slate-500">No steps planned.</p>
      ) : null}
      <div className="mt-3 flex gap-2">
        {onApprove ? (
          <button
            type="button"
            disabled={busy === true}
            onClick={onApprove}
            aria-label="Approve plan"
            className="rounded-md border border-emerald-400/30 bg-emerald-400/10 px-2 py-1 text-xs text-emerald-200 disabled:opacity-50"
          >
            Approve plan
          </button>
        ) : null}
        {onReject ? (
          <button
            type="button"
            disabled={busy === true}
            onClick={onReject}
            aria-label="Reject plan"
            className="rounded-md border border-rose-400/30 bg-rose-400/10 px-2 py-1 text-xs text-rose-200 disabled:opacity-50"
          >
            Reject plan
          </button>
        ) : null}
      </div>
    </GlassPanel>
  );
}
