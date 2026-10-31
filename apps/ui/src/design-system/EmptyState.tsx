import type { ReactNode } from "react";

interface EmptyStateProps {
  title: string;
  message: string;
  action?: ReactNode;
}

export function EmptyState({ title, message, action }: EmptyStateProps) {
  return (
    <div className="flex flex-col items-center justify-center gap-2 px-6 py-12 text-center">
      <div
        aria-hidden="true"
        className="mb-2 size-12 rounded-full border border-cyan-300/30 bg-cyan-400/10 shadow-[0_0_24px_rgb(34_211_238/0.22)]"
      />
      <h3 className="text-sm font-semibold text-slate-100">{title}</h3>
      <p className="max-w-sm text-sm text-slate-400">{message}</p>
      {action}
    </div>
  );
}
