import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { ApprovalRequest } from "@talos/contracts";
import { DiffViewer } from "./DiffViewer.js";
import { useApprovalStore } from "../../stores/approvalStore.js";

const EFFECT_STYLE: Record<ApprovalRequest["effectClass"], string> = {
  READ: "bg-emerald-500/15 text-emerald-300 border-emerald-400/30",
  LOCAL_WRITE: "bg-cyan-500/15 text-cyan-300 border-cyan-400/30",
  EXTERNAL_WRITE: "bg-amber-500/15 text-amber-300 border-amber-400/30",
  DESTRUCTIVE: "bg-rose-500/20 text-rose-200 border-rose-400/40",
  FINANCIAL: "bg-purple-500/15 text-purple-300 border-purple-400/30",
  PRIVILEGED: "bg-red-500/20 text-red-200 border-red-400/40",
};

const EFFECT_ICON: Record<ApprovalRequest["effectClass"], string> = {
  READ: "R",
  LOCAL_WRITE: "W",
  EXTERNAL_WRITE: "E",
  DESTRUCTIVE: "D",
  FINANCIAL: "F",
  PRIVILEGED: "P",
};

const REVERSIBILITY_LABEL: Record<ApprovalRequest["reversibility"], string> = {
  reversible: "reversible (checkpoint available)",
  irreversible: "irreversible (compensation plan shown)",
  unknown: "unknown (treat as irreversible)",
};

const RISK_TONE: Record<ApprovalRequest["riskLevel"], string> = {
  low: "text-emerald-300",
  medium: "text-cyan-300",
  high: "text-amber-300",
  irreversible: "text-rose-300",
};

function isHighRisk(approval: ApprovalRequest): boolean {
  return (
    approval.riskLevel === "high" ||
    approval.riskLevel === "irreversible" ||
    approval.reversibility === "irreversible" ||
    approval.effectClass === "DESTRUCTIVE" ||
    approval.effectClass === "FINANCIAL" ||
    approval.effectClass === "PRIVILEGED"
  );
}

function countdown(expiresAt: string, now: number): string {
  const ms = Date.parse(expiresAt) - now;
  if (Number.isNaN(ms) || ms <= 0) return "expired";
  const s = Math.floor(ms / 1000);
  return `${Math.floor(s / 60)}m ${s % 60}s left`;
}

function PayloadSection({ approval }: { approval: ApprovalRequest }) {
  const payload = approval.payload as Record<string, unknown> | null;
  if (approval.payloadKind === "file_edit") {
    return (
      <section aria-label="Payload and diff">
        <h3 className="mb-1 text-xs font-semibold uppercase tracking-wider text-slate-400">
          Payload / Diff — exact content to be written
        </h3>
        <DiffViewer
          files={approval.diffFiles}
          original={approval.originalContent}
          proposed={approval.proposedContent}
          target={approval.target}
        />
      </section>
    );
  }
  if (approval.payloadKind === "shell") {
    const command =
      (payload?.["command"] as string | undefined) ??
      (payload?.["cmd"] as string | undefined) ??
      JSON.stringify(approval.payload);
    return (
      <section aria-label="Payload: exact command">
        <h3 className="mb-1 text-xs font-semibold uppercase tracking-wider text-slate-400">
          Payload — exact command to execute
        </h3>
        <pre
          aria-label="Exact shell command"
          className="overflow-x-auto rounded-md border border-white/10 bg-black/50 p-3 font-mono text-xs text-amber-200"
        >
          {command}
        </pre>
        <details className="mt-1 text-xs text-slate-500">
          <summary className="cursor-pointer hover:text-slate-300">
            Full parameters (JSON)
          </summary>
          <pre className="mt-1 overflow-x-auto rounded bg-black/40 p-2 font-mono text-[11px]">
            {JSON.stringify(approval.payload, null, 2)}
          </pre>
        </details>
      </section>
    );
  }
  if (approval.payloadKind === "browser") {
    const url =
      (payload?.["url"] as string | undefined) ??
      (payload?.["target"] as string | undefined) ??
      approval.target;
    const selector = (payload?.["selector"] as string | undefined) ?? "—";
    const action =
      (payload?.["action"] as string | undefined) ??
      (payload?.["op"] as string | undefined) ??
      "browser action";
    return (
      <section aria-label="Payload: browser action">
        <h3 className="mb-1 text-xs font-semibold uppercase tracking-wider text-slate-400">
          Payload — browser action
        </h3>
        <dl className="grid grid-cols-[110px_1fr] gap-1 rounded-md border border-white/10 bg-black/40 p-3 font-mono text-xs">
          <dt className="text-slate-500">URL</dt>
          <dd className="break-all text-cyan-200">{url}</dd>
          <dt className="text-slate-500">Selector</dt>
          <dd className="break-all text-slate-200">{selector}</dd>
          <dt className="text-slate-500">Action</dt>
          <dd className="text-slate-200">{action}</dd>
        </dl>
        <details className="mt-1 text-xs text-slate-500">
          <summary className="cursor-pointer hover:text-slate-300">
            Full parameters (JSON)
          </summary>
          <pre className="mt-1 overflow-x-auto rounded bg-black/40 p-2 font-mono text-[11px]">
            {JSON.stringify(approval.payload, null, 2)}
          </pre>
        </details>
      </section>
    );
  }
  if (approval.payloadKind === "network") {
    const method = (payload?.["method"] as string | undefined) ?? "POST";
    const endpoint =
      (payload?.["endpoint"] as string | undefined) ??
      (payload?.["url"] as string | undefined) ??
      approval.target;
    return (
      <section aria-label="Payload: network request">
        <h3 className="mb-1 text-xs font-semibold uppercase tracking-wider text-slate-400">
          Payload — network request preview
        </h3>
        <dl className="grid grid-cols-[110px_1fr] gap-1 rounded-md border border-white/10 bg-black/40 p-3 font-mono text-xs">
          <dt className="text-slate-500">Method</dt>
          <dd className="text-slate-200">{method}</dd>
          <dt className="text-slate-500">Endpoint</dt>
          <dd className="break-all text-cyan-200">{endpoint}</dd>
        </dl>
        <pre
          aria-label="Request payload preview"
          className="mt-1 max-h-40 overflow-auto rounded-md border border-white/10 bg-black/50 p-3 font-mono text-[11px] text-slate-300"
        >
          {JSON.stringify(approval.payload, null, 2)}
        </pre>
      </section>
    );
  }
  return (
    <section aria-label="Payload">
      <h3 className="mb-1 text-xs font-semibold uppercase tracking-wider text-slate-400">
        Payload — exact parameters
      </h3>
      <pre className="max-h-48 overflow-auto rounded-md border border-white/10 bg-black/50 p-3 font-mono text-xs text-slate-300">
        {JSON.stringify(approval.payload, null, 2)}
      </pre>
    </section>
  );
}

/**
 * Full-screen HITL approval sheet. Non-dismissable: the user must Approve,
 * Deny, or Modify. Keyboard: Enter = approve, Escape = deny, M = modify
 * (ignored while typing in an input). Focus is trapped while open.
 *
 * Remounts per approval (`key`) so editable state always starts from the
 * incoming payload without a reset effect.
 */
export function ApprovalSheet({
  approval,
  queuePosition,
  queueTotal,
  onApprove,
  onDeny,
  onModify,
  onNext,
  onPrev,
  busy,
  error,
}: {
  approval: ApprovalRequest;
  queuePosition: number;
  queueTotal: number;
  onApprove: (dontAskAgain: boolean) => void;
  onDeny: (reason: string) => void;
  onModify: (params: unknown) => void;
  onNext: () => void;
  onPrev: () => void;
  busy: boolean;
  error: string | null;
}) {
  return (
    <ApprovalSheetInner
      key={approval.approvalId}
      approval={approval}
      queuePosition={queuePosition}
      queueTotal={queueTotal}
      onApprove={onApprove}
      onDeny={onDeny}
      onModify={onModify}
      onNext={onNext}
      onPrev={onPrev}
      busy={busy}
      error={error}
    />
  );
}

function ApprovalSheetInner({
  approval,
  queuePosition,
  queueTotal,
  onApprove,
  onDeny,
  onModify,
  onNext,
  onPrev,
  busy,
  error,
}: {
  approval: ApprovalRequest;
  queuePosition: number;
  queueTotal: number;
  onApprove: (dontAskAgain: boolean) => void;
  onDeny: (reason: string) => void;
  onModify: (params: unknown) => void;
  onNext: () => void;
  onPrev: () => void;
  busy: boolean;
  error: string | null;
}) {
  const dialogRef = useRef<HTMLDivElement>(null);
  const approveRef = useRef<HTMLButtonElement>(null);
  const [modifying, setModifying] = useState(false);
  const [draft, setDraft] = useState(() =>
    JSON.stringify(approval.payload, null, 2),
  );
  const [draftError, setDraftError] = useState<string | null>(null);
  const [dontAskAgain, setDontAskAgain] = useState(false);
  const [denyReason, setDenyReason] = useState("");
  const [now, setNow] = useState(() => Date.now());
  const dangerous = isHighRisk(approval);

  useEffect(() => {
    const timer = setInterval(() => {
      setNow(Date.now());
    }, 1_000);
    return () => {
      clearInterval(timer);
    };
  }, []);

  useEffect(() => {
    approveRef.current?.focus();
  }, [approval.approvalId]);

  const confirmModify = useCallback(() => {
    try {
      const parsed: unknown = JSON.parse(draft);
      setDraftError(null);
      onModify(parsed);
    } catch (err) {
      setDraftError(err instanceof Error ? err.message : "Invalid JSON");
    }
  }, [draft, onModify]);

  const handleDeny = useCallback(() => {
    onDeny(denyReason.trim() === "" ? "denied by reviewer" : denyReason.trim());
  }, [denyReason, onDeny]);

  // Keyboard shortcuts + focus trap (non-dismissable: no bare close).
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      const typing =
        target !== null &&
        (target.tagName === "INPUT" ||
          target.tagName === "TEXTAREA" ||
          target.tagName === "SELECT" ||
          target.isContentEditable);
      if (event.key === "Tab") {
        const root = dialogRef.current;
        if (!root) return;
        const focusables = [...root.querySelectorAll<HTMLElement>(
          'button:not([disabled]), [href], input, select, textarea, [tabindex]:not([tabindex="-1"])',
        )].filter((el) => !el.hasAttribute("disabled") && el.offsetParent !== null);
        if (focusables.length === 0) return;
        const first = focusables[0];
        const last = focusables[focusables.length - 1];
        if (first === undefined || last === undefined) return;
        if (event.shiftKey && document.activeElement === first) {
          event.preventDefault();
          last.focus();
        } else if (!event.shiftKey && document.activeElement === last) {
          event.preventDefault();
          first.focus();
        }
        return;
      }
      if (typing) return;
      if (event.key === "Enter" && !modifying) {
        event.preventDefault();
        onApprove(dontAskAgain);
      } else if (event.key === "Escape") {
        event.preventDefault();
        handleDeny();
      } else if (event.key === "m" || event.key === "M") {
        event.preventDefault();
        setModifying((v) => !v);
      }
    };
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("keydown", onKey);
    };
  }, [dontAskAgain, handleDeny, modifying, onApprove]);

  const expiry = useMemo(
    () => countdown(approval.expiresAt, now),
    [approval.expiresAt, now],
  );

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4"
      role="presentation"
      aria-hidden={false}
    >
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="approval-title"
        aria-describedby="approval-summary"
        className={`glass-panel max-h-[92vh] w-full max-w-3xl overflow-y-auto p-5 ${
          dangerous ? "ring-2 ring-rose-500/60" : "ring-1 ring-white/10"
        }`}
      >
        {/* Most dangerous info first: risk banner */}
        <div
          role="alert"
          aria-label={`Risk ${approval.riskLevel}, ${approval.effectClass}`}
          className={`mb-3 flex flex-wrap items-center gap-2 rounded-md border px-3 py-2 text-sm font-semibold ${
            dangerous
              ? "border-rose-400/40 bg-rose-500/10 text-rose-200"
              : "border-white/10 bg-white/5 text-slate-200"
          }`}
        >
          <span
            aria-hidden="true"
            className={`flex h-7 w-7 items-center justify-center rounded border font-mono text-sm font-bold ${EFFECT_STYLE[approval.effectClass]}`}
          >
            {EFFECT_ICON[approval.effectClass]}
          </span>
          <span>{approval.effectClass}</span>
          <span aria-hidden="true">·</span>
          <span className={RISK_TONE[approval.riskLevel]}>
            {approval.riskLevel.toUpperCase()} risk
          </span>
          <span aria-hidden="true">·</span>
          <span className="font-normal text-slate-300">
            {REVERSIBILITY_LABEL[approval.reversibility]}
          </span>
          <span className="ml-auto font-mono text-xs font-normal text-slate-400">
            {queuePosition + 1} of {queueTotal} · {expiry}
          </span>
        </div>

        <div className="mb-1 flex items-start justify-between gap-3">
          <div>
            <h2 id="approval-title" className="font-display text-lg font-semibold">
              Approval requested
            </h2>
            <p className="font-mono text-xs text-slate-500">
              {approval.approvalId} · task {approval.taskId} · agent {approval.agentId}
            </p>
          </div>
          {queueTotal > 1 ? (
            <div className="flex shrink-0 gap-1" aria-label="Approval queue navigation">
              <button
                type="button"
                onClick={onPrev}
                aria-label="Previous approval"
                className="rounded-md border border-white/10 px-2 py-1 text-xs hover:bg-white/5"
              >
                ← Prev
              </button>
              <button
                type="button"
                onClick={onNext}
                aria-label="Next approval"
                className="rounded-md border border-white/10 px-2 py-1 text-xs hover:bg-white/5"
              >
                Next →
              </button>
            </div>
          ) : null}
        </div>

        <div className="flex flex-col gap-4">
          <section aria-label="Action summary">
            <h3 className="mb-1 text-xs font-semibold uppercase tracking-wider text-slate-400">
              Action summary
            </h3>
            <p id="approval-summary" className="text-sm text-slate-100">
              {approval.summary}
            </p>
          </section>

          <section aria-label="Target resource">
            <h3 className="mb-1 text-xs font-semibold uppercase tracking-wider text-slate-400">
              Target — exact resource
            </h3>
            <p className="break-all rounded-md border border-white/10 bg-black/40 p-2 font-mono text-xs text-cyan-200">
              {approval.target}
            </p>
            <p className="mt-1 font-mono text-[11px] text-slate-500">
              capability {approval.capability} · action {approval.actionId}
            </p>
          </section>

          <PayloadSection approval={approval} />

          <section aria-label="Risk assessment">
            <h3 className="mb-1 text-xs font-semibold uppercase tracking-wider text-slate-400">
              Risk assessment
            </h3>
            <p className="text-sm text-slate-200">{approval.policyReason}</p>
            {approval.requiredChecks.length > 0 ? (
              <ul className="mt-1 flex flex-wrap gap-1" aria-label="Required checks">
                {approval.requiredChecks.map((check) => (
                  <li
                    key={check}
                    className="rounded bg-white/5 px-1.5 py-0.5 font-mono text-[11px] text-slate-300"
                  >
                    {check}
                  </li>
                ))}
              </ul>
            ) : null}
          </section>

          {approval.compensation.length > 0 ? (
            <section aria-label="Compensation plan">
              <h3 className="mb-1 text-xs font-semibold uppercase tracking-wider text-slate-400">
                Compensation plan — shown because this is{" "}
                {approval.reversibility}
              </h3>
              <ol className="flex flex-col gap-1">
                {approval.compensation.map((step) => (
                  <li
                    key={step.order}
                    className="flex gap-2 rounded-md border border-white/10 bg-black/40 p-2 text-xs"
                  >
                    <span className="font-mono text-slate-500">{step.order + 1}.</span>
                    <div>
                      <p className="font-mono text-slate-300">{step.actionType}</p>
                      <p className="text-slate-400">{step.description}</p>
                    </div>
                  </li>
                ))}
              </ol>
            </section>
          ) : null}

          <section aria-label="Cost estimate and scope">
            <h3 className="mb-1 text-xs font-semibold uppercase tracking-wider text-slate-400">
              Cost estimate · Scope &amp; duration
            </h3>
            <p className="text-xs text-slate-300">
              {approval.costEstimate != null
                ? `≈ ${approval.costEstimate.tokens} tokens (~$${approval.costEstimate.estimatedUsd.toFixed(5)})`
                : "No cost estimate for this action."}
            </p>
            <p className="mt-1 text-xs text-slate-400">{approval.scope}</p>
            <p className="mt-1 font-mono text-[11px] text-slate-500">
              Expires {approval.expiresAt} (5-minute default timeout)
            </p>
          </section>

          {modifying ? (
            <section aria-label="Modify parameters">
              <h3 className="mb-1 text-xs font-semibold uppercase tracking-wider text-amber-300">
                Modify — edit parameters, then re-evaluate policy
              </h3>
              <textarea
                aria-label="Modified parameters as JSON"
                value={draft}
                onChange={(e) => {
                  setDraft(e.target.value);
                }}
                rows={8}
                spellCheck={false}
                className="w-full rounded-md border border-amber-400/30 bg-black/50 p-2 font-mono text-xs text-slate-200"
              />
              {draftError !== null ? (
                <p role="alert" className="mt-1 text-xs text-rose-300">
                  {draftError}
                </p>
              ) : null}
              <div className="mt-2 flex gap-2">
                <button
                  type="button"
                  onClick={confirmModify}
                  disabled={busy}
                  className="rounded-md bg-amber-400/20 px-3 py-1.5 text-sm text-amber-200 hover:bg-amber-400/30 disabled:opacity-50"
                >
                  Submit modification
                </button>
                <button
                  type="button"
                  onClick={() => {
                    setModifying(false);
                  }}
                  className="rounded-md border border-white/10 px-3 py-1.5 text-sm text-slate-300 hover:bg-white/5"
                >
                  Cancel edit
                </button>
              </div>
            </section>
          ) : null}

          <section aria-label="Deny reason">
            <label
              htmlFor="approval-deny-reason"
              className="mb-1 block text-xs font-semibold uppercase tracking-wider text-slate-400"
            >
              Deny reason (optional, sent with the denial)
            </label>
            <input
              id="approval-deny-reason"
              value={denyReason}
              onChange={(e) => {
                setDenyReason(e.target.value);
              }}
              placeholder="Why is this denied?"
              className="w-full rounded-md border border-white/10 bg-black/30 px-2 py-1.5 text-sm"
            />
          </section>

          {error !== null ? (
            <p role="alert" className="text-sm text-rose-300">
              {error}
            </p>
          ) : null}

          <div className="flex flex-wrap items-center gap-2 border-t border-white/10 pt-3">
            <button
              ref={approveRef}
              type="button"
              onClick={() => {
                onApprove(dontAskAgain);
              }}
              disabled={busy}
              aria-label="Approve action (Enter)"
              data-autofocus
              className="rounded-md bg-emerald-500 px-4 py-2 text-sm font-semibold text-black hover:bg-emerald-400 disabled:opacity-50"
            >
              Approve ⏎
            </button>
            <button
              type="button"
              onClick={handleDeny}
              disabled={busy}
              aria-label="Deny action (Escape)"
              className="rounded-md bg-rose-600 px-4 py-2 text-sm font-semibold text-white hover:bg-rose-500 disabled:opacity-50"
            >
              Deny ⎋
            </button>
            <button
              type="button"
              onClick={() => {
                setModifying((v) => !v);
              }}
              aria-label="Modify parameters (M)"
              className="rounded-md border border-amber-400/40 bg-amber-400/10 px-4 py-2 text-sm text-amber-200 hover:bg-amber-400/20"
            >
              Modify (M)
            </button>
            <label className="ml-auto flex cursor-pointer items-center gap-2 text-xs text-slate-400">
              <input
                type="checkbox"
                checked={dontAskAgain}
                onChange={(e) => {
                  setDontAskAgain(e.target.checked);
                }}
                aria-label="Don't ask again for this action type in this task"
              />
              Don&apos;t ask again for this action type (this task only)
            </label>
          </div>
          <p className="font-mono text-[11px] text-slate-600">
            Shortcuts: Enter = approve · Escape = deny · M = modify. Focus is
            trapped until you decide. High-risk items always need individual
            review.
          </p>
        </div>
      </div>
    </div>
  );
}

/** Overlay host: renders the sheet for the active pending approval. */
export function ApprovalOverlay({
  onApprove,
  onDeny,
  onModify,
  busy,
  error,
}: {
  onApprove: (approval: ApprovalRequest, dontAskAgain: boolean) => void;
  onDeny: (approval: ApprovalRequest, reason: string) => void;
  onModify: (approval: ApprovalRequest, params: unknown) => void;
  busy: boolean;
  error: string | null;
}) {
  const pending = useApprovalStore((s) => s.pending);
  const sheetOpen = useApprovalStore((s) => s.sheetOpen);
  const activeId = useApprovalStore((s) => s.activeApprovalId);
  const setActive = useApprovalStore((s) => s.setActive);
  const active =
    pending.find((a) => a.approvalId === activeId) ?? pending[0] ?? null;
  if (!sheetOpen || active === null) return null;
  const idx = pending.findIndex((a) => a.approvalId === active.approvalId);
  return (
    <ApprovalSheet
      approval={active}
      queuePosition={Math.max(idx, 0)}
      queueTotal={pending.length}
      onApprove={(dontAsk) => {
        onApprove(active, dontAsk);
      }}
      onDeny={(reason) => {
        onDeny(active, reason);
      }}
      onModify={(params) => {
        onModify(active, params);
      }}
      onNext={() => {
        const next = pending[(idx + 1) % pending.length];
        if (next) setActive(next.approvalId);
      }}
      onPrev={() => {
        const prev = pending[(idx - 1 + pending.length) % pending.length];
        if (prev) setActive(prev.approvalId);
      }}
      busy={busy}
      error={error}
    />
  );
}
