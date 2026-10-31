import type { ActivityEvent } from "../types/index.js";
import { formatTime } from "../lib/format.js";
import { cn } from "../lib/cn.js";

const TONE_DOT: Record<ActivityEvent["tone"], string> = {
  info: "bg-cyan-300",
  success: "bg-emerald-400",
  warning: "bg-amber-300",
  error: "bg-rose-400",
};

export function LogStream({ events }: { events: ActivityEvent[] }) {
  return (
    <ol
      role="log"
      aria-label="Recent activity"
      className="flex max-h-72 flex-col gap-1 overflow-y-auto font-mono text-xs"
    >
      {events.map((event) => (
        <li
          key={event.id}
          className="flex items-baseline gap-3 rounded-md px-2 py-1.5 hover:bg-white/5"
        >
          <span className="shrink-0 text-slate-500">
            {formatTime(event.at)}
          </span>
          <span
            aria-hidden="true"
            className={cn(
              "size-1.5 shrink-0 rounded-full",
              TONE_DOT[event.tone],
            )}
          />
          <span className="shrink-0 text-violet-300">{event.actor}</span>
          <span className="text-slate-300">{event.message}</span>
        </li>
      ))}
    </ol>
  );
}
