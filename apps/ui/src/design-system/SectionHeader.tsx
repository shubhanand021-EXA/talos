import type { ReactNode } from "react";

interface SectionHeaderProps {
  title: string;
  subtitle?: string;
  action?: ReactNode;
}

export function SectionHeader({ title, subtitle, action }: SectionHeaderProps) {
  return (
    <header className="mb-4 flex items-start justify-between gap-4">
      <div>
        <h2 className="font-display text-lg font-semibold tracking-tight text-slate-100">
          {title}
        </h2>
        {subtitle ? (
          <p className="mt-1 text-sm text-slate-400">{subtitle}</p>
        ) : null}
      </div>
      {action}
    </header>
  );
}
