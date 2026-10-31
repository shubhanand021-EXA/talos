import type { ReactNode } from "react";
import { cn } from "../lib/cn.js";

interface GlassPanelProps {
  children: ReactNode;
  className?: string;
  label?: string;
}

export function GlassPanel({ children, className, label }: GlassPanelProps) {
  return (
    <section aria-label={label} className={cn("glass-panel p-5", className)}>
      {children}
    </section>
  );
}
