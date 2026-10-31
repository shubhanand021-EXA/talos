import type { ReactNode } from "react";

interface TooltipProps {
  label: string;
  children: ReactNode;
}

/** CSS-only tooltip: hover or keyboard-focus reveals the label. */
export function Tooltip({ label, children }: TooltipProps) {
  return (
    <span
      className="group relative inline-flex"
      tabIndex={0}
      aria-label={label}
    >
      {children}
      <span
        role="tooltip"
        className="pointer-events-none absolute bottom-full left-1/2 z-50 mb-2 -translate-x-1/2 whitespace-nowrap rounded-md border border-white/10 bg-void-900 px-2 py-1 text-xs text-slate-200 opacity-0 shadow-lg transition-opacity duration-fast group-hover:opacity-100 group-focus-visible:opacity-100"
      >
        {label}
      </span>
    </span>
  );
}
