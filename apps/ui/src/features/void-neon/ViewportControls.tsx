import type { SceneNodeKind } from "@talos/contracts";
import { KIND_FILTER_GROUPS } from "./topology.js";

export interface ReplayEvent {
  id: string;
  label: string;
  taskId?: string | undefined;
}

interface ViewportControlsProps {
  visibleKinds: ReadonlySet<SceneNodeKind>;
  onToggleKind: (kind: SceneNodeKind) => void;
  focusMode: boolean;
  onToggleFocus: () => void;
  hasSelection: boolean;
  replayEvents: ReplayEvent[];
  replayIndex: number | null;
  onReplayChange: (index: number | null) => void;
}

/**
 * Viewport controls: kind filters, focus mode, and a lightweight replay
 * slider over recent task-scoped events. DOM only — scene stays allocation
 * free while scrubbing.
 */
export function ViewportControls({
  visibleKinds,
  onToggleKind,
  focusMode,
  onToggleFocus,
  hasSelection,
  replayEvents,
  replayIndex,
  onReplayChange,
}: ViewportControlsProps) {
  const current =
    replayIndex !== null ? (replayEvents[replayIndex] ?? null) : null;

  return (
    <div
      aria-label="Viewport controls"
      className="pointer-events-auto absolute bottom-10 left-3 flex max-w-[240px] flex-col gap-2 rounded-control border border-white/10 bg-void-950/80 p-2 backdrop-blur"
    >
      <p className="font-mono text-[10px] uppercase tracking-widest text-slate-500">
        layers
      </p>
      <div className="flex flex-wrap gap-1">
        {KIND_FILTER_GROUPS.map(({ kind, label }) => {
          const on = visibleKinds.has(kind);
          return (
            <button
              key={kind}
              type="button"
              aria-pressed={on}
              onClick={() => {
                onToggleKind(kind);
              }}
              className={`rounded-full border px-2 py-0.5 font-mono text-[10px] ${
                on
                  ? "border-cyan-300/50 bg-cyan-950/60 text-cyan-200"
                  : "border-white/10 bg-black/30 text-slate-500"
              }`}
            >
              {label}
            </button>
          );
        })}
      </div>
      <div className="flex items-center gap-2">
        <button
          type="button"
          aria-pressed={focusMode}
          disabled={!hasSelection}
          onClick={onToggleFocus}
          title={
            hasSelection
              ? "Dim everything outside the selection neighborhood"
              : "Select a node first"
          }
          className={`rounded-full border px-2 py-0.5 font-mono text-[10px] disabled:opacity-40 ${
            focusMode
              ? "border-cyan-300/50 bg-cyan-950/60 text-cyan-200"
              : "border-white/10 bg-black/30 text-slate-400"
          }`}
        >
          focus
        </button>
        <button
          type="button"
          aria-pressed={replayIndex === null}
          onClick={() => {
            onReplayChange(null);
          }}
          className="rounded-full border border-white/10 bg-black/30 px-2 py-0.5 font-mono text-[10px] text-slate-400"
        >
          live
        </button>
      </div>
      {replayEvents.length > 0 ? (
        <div className="flex flex-col gap-1">
          <label
            htmlFor="scene-replay"
            className="font-mono text-[10px] uppercase tracking-widest text-slate-500"
          >
            replay
          </label>
          <input
            id="scene-replay"
            type="range"
            min={0}
            max={replayEvents.length - 1}
            value={replayIndex ?? replayEvents.length - 1}
            onChange={(event) => {
              onReplayChange(Number(event.target.value));
            }}
            className="w-full accent-cyan-300"
          />
          <p className="truncate font-mono text-[10px] text-slate-400">
            {current === null ? "live edge" : current.label}
          </p>
        </div>
      ) : null}
    </div>
  );
}
