import type { ButtonHTMLAttributes, ReactNode } from "react";
import { cn } from "../lib/cn.js";

type NeonVariant = "primary" | "ghost" | "danger";

interface NeonButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  children: ReactNode;
  variant?: NeonVariant;
  loading?: boolean;
}

const VARIANTS: Record<NeonVariant, string> = {
  primary:
    "bg-cyan-400/15 text-cyan-200 border-cyan-300/40 hover:bg-cyan-400/25 hover:shadow-[0_0_24px_rgb(34_211_238/0.22)]",
  ghost:
    "bg-transparent text-slate-200 border-slate-500/40 hover:bg-white/5 hover:border-slate-400/60",
  danger:
    "bg-rose-500/15 text-rose-200 border-rose-400/40 hover:bg-rose-500/25",
};

export function NeonButton({
  children,
  variant = "primary",
  loading = false,
  disabled,
  className,
  type,
  ...rest
}: NeonButtonProps) {
  const busy = loading || disabled;
  return (
    <button
      type={type ?? "button"}
      disabled={busy}
      aria-busy={loading}
      className={cn(
        "inline-flex items-center justify-center gap-2 rounded-[0.625rem] border px-4 py-2 text-sm font-medium",
        "transition-all duration-160 focus-visible:outline-2 focus-visible:outline-cyan-300",
        "disabled:cursor-not-allowed disabled:opacity-45",
        VARIANTS[variant],
        className,
      )}
      {...rest}
    >
      {loading ? (
        <span
          aria-hidden="true"
          className="inline-block size-4 animate-spin rounded-full border-2 border-current border-t-transparent"
        />
      ) : null}
      {children}
    </button>
  );
}
