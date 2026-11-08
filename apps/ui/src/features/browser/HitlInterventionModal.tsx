import { useState } from "react";
import {
  GlassPanel,
  NeonButton,
  StatusPill,
} from "../../design-system/index.js";
import type { BrowserHitlRequiredPayload } from "../../bridge/index.js";

interface HitlInterventionModalProps {
  /** Paused intervention the modal renders (linkage + reason only). */
  intervention: BrowserHitlRequiredPayload;
  /** Foreground Playwright window state hint for the user. */
  onResume: (interventionId: string) => Promise<void>;
  onDismiss: (interventionId: string) => Promise<void>;
  onClose: () => void;
}

/**
 * `Talos requires manual browser intervention`: the Browser Agent paused on
 * a CAPTCHA/bot wall or login gate. The user solves the challenge in the
 * foreground Playwright window (same Gateway `user_data_dir`), then clicks
 * Resume so the agent continues from the paused state — or Dismiss to route
 * the agent around the wall.
 */
export function HitlInterventionModal({
  intervention,
  onResume,
  onDismiss,
  onClose,
}: HitlInterventionModalProps) {
  const [busy, setBusy] = useState<"resume" | "dismiss" | null>(null);
  const [error, setError] = useState<string | null>(null);

  const act = async (kind: "resume" | "dismiss") => {
    setBusy(kind);
    setError(null);
    try {
      if (kind === "resume") {
        await onResume(intervention.interventionId);
      } else {
        await onDismiss(intervention.interventionId);
      }
      onClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Resolution failed");
    } finally {
      setBusy(null);
    }
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
      role="presentation"
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Talos requires manual browser intervention"
        className="glass-panel w-full max-w-lg p-5"
      >
        <div className="mb-3 flex items-center justify-between gap-3">
          <h2 className="font-display text-base font-semibold">
            Talos requires manual browser intervention
          </h2>
          <StatusPill tone="warn">
            {intervention.kind === "auth_required"
              ? "login wall"
              : "captcha wall"}
          </StatusPill>
        </div>
        <GlassPanel label="Paused execution">
          <dl className="flex flex-col gap-1 text-xs">
            <div className="flex justify-between gap-3">
              <dt className="text-slate-500">Task</dt>
              <dd className="font-mono text-slate-200">
                {intervention.taskId}
              </dd>
            </div>
            {intervention.url !== null && intervention.url !== undefined ? (
              <div className="flex justify-between gap-3">
                <dt className="text-slate-500">Page</dt>
                <dd className="break-all font-mono text-slate-200">
                  {intervention.url}
                </dd>
              </div>
            ) : null}
            <div className="flex justify-between gap-3">
              <dt className="text-slate-500">Reason</dt>
              <dd className="font-mono text-slate-200">
                {intervention.reason}
              </dd>
            </div>
            <div className="flex justify-between gap-3">
              <dt className="text-slate-500">Intervention</dt>
              <dd className="font-mono text-slate-200">
                {intervention.interventionId}
              </dd>
            </div>
          </dl>
        </GlassPanel>
        <p className="mt-3 text-xs text-slate-400">
          Solve the challenge or log in inside the foreground browser window (it
          shares the agent&apos;s saved profile), then click Resume — the agent
          continues from the paused state. Dismiss routes the agent around the
          wall instead.
        </p>
        {error !== null ? (
          <p role="alert" className="mt-2 text-xs text-amber-300">
            {error}
          </p>
        ) : null}
        <div className="mt-4 flex justify-end gap-2">
          <NeonButton
            onClick={() => {
              void act("dismiss");
            }}
            loading={busy === "dismiss"}
          >
            Dismiss
          </NeonButton>
          <NeonButton
            onClick={() => {
              void act("resume");
            }}
            loading={busy === "resume"}
          >
            Resume
          </NeonButton>
        </div>
      </div>
    </div>
  );
}
