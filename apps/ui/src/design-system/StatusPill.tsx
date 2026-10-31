import { cn } from "../lib/cn.js";

export type PillTone = "ok" | "info" | "warn" | "error" | "neutral";

interface StatusPillProps {
  tone: PillTone;
  children: string;
}

const TONES: Record<PillTone, string> = {
  ok: "border-emerald-400/40 bg-emerald-400/10 text-emerald-200",
  info: "border-cyan-300/40 bg-cyan-400/10 text-cyan-200",
  warn: "border-amber-300/40 bg-amber-400/10 text-amber-200",
  error: "border-rose-400/40 bg-rose-500/10 text-rose-200",
  neutral: "border-slate-500/40 bg-white/5 text-slate-300",
};

export function StatusPill({ tone, children }: StatusPillProps) {
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1.5 rounded-full border px-2.5 py-0.5 text-xs font-medium",
        TONES[tone],
      )}
    >
      <span aria-hidden="true" className="size-1.5 rounded-full bg-current" />
      {children}
    </span>
  );
}
