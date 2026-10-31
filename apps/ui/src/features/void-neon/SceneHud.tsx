import { StatusPill } from "../../design-system/StatusPill.js";
import type { SceneNode } from "@talos/contracts";
import { useSceneSettings } from "./scene-settings.js";
import { SCENE_QUALITIES, type SceneQuality } from "./types.js";
import { describeNode } from "./topology.js";

export interface SceneCounts {
  tasksActive: number;
  waitingPolicy: number;
  browserEvents: number;
  computerEvents: number;
}

export interface SceneSafety {
  stopped: boolean;
  enabled: boolean;
}

interface SceneHudProps {
  activeCount: number;
  selected: SceneNode | null;
  hovered: SceneNode | null;
  fps: number;
  counts: SceneCounts;
  safety: SceneSafety | null;
  ledgerHealthy: boolean;
  evidenceVersion: number;
}

/** Glass HUD overlay: nodes, selection, live counts, safety. DOM only. */
export function SceneHud({
  activeCount,
  selected,
  hovered,
  fps,
  counts,
  safety,
  ledgerHealthy,
  evidenceVersion,
}: SceneHudProps) {
  const { quality, setQuality } = useSceneSettings();
  const tooltip = hovered ?? selected;

  return (
    <div className="pointer-events-none absolute inset-0 flex flex-col justify-between p-3">
      {/* Emergency stop: global red safety overlay (DOM, always readable). */}
      {safety?.stopped === true ? (
        <div
          role="alert"
          aria-label="Emergency stop active"
          className="pointer-events-none absolute inset-0 border-2 border-red-500/70 bg-red-950/25"
        >
          <p className="absolute left-1/2 top-2 -translate-x-1/2 rounded bg-red-950/90 px-3 py-1 font-mono text-[11px] uppercase tracking-widest text-red-300">
            emergency stop · computer-use halted
          </p>
        </div>
      ) : null}
      <div className="flex items-start justify-between gap-2">
        <div className="pointer-events-auto flex flex-wrap items-center gap-2">
          <StatusPill tone="info">{`${activeCount} nodes live`}</StatusPill>
          {selected !== null ? (
            <StatusPill tone="ok">{selected.label}</StatusPill>
          ) : (
            <StatusPill tone="neutral">no selection</StatusPill>
          )}
          <StatusPill tone={counts.waitingPolicy > 0 ? "warn" : "neutral"}>
            {`policy gate ${counts.waitingPolicy}`}
          </StatusPill>
          <StatusPill tone="neutral">
            {`tasks ${counts.tasksActive}`}
          </StatusPill>
          <StatusPill tone="neutral">
            {`browser ${counts.browserEvents} · computer ${counts.computerEvents}`}
          </StatusPill>
          {safety === null ? (
            <StatusPill tone="neutral">safety unknown</StatusPill>
          ) : safety.stopped ? (
            <StatusPill tone="error">STOPPED</StatusPill>
          ) : (
            <StatusPill tone={safety.enabled ? "warn" : "ok"}>
              {safety.enabled ? "computer armed" : "computer safe"}
            </StatusPill>
          )}
          <StatusPill tone={ledgerHealthy ? "ok" : "error"}>
            {`ledger ${ledgerHealthy ? "verified" : "broken"} · v${evidenceVersion}`}
          </StatusPill>
        </div>
        <div className="pointer-events-auto flex items-center gap-2 rounded-control border border-white/10 bg-void-950/70 px-2 py-1 backdrop-blur">
          <label htmlFor="scene-quality" className="text-[11px] text-slate-400">
            Quality
          </label>
          <select
            id="scene-quality"
            value={quality}
            onChange={(event) => {
              setQuality(event.target.value as SceneQuality);
            }}
            className="bg-transparent font-mono text-[11px] text-cyan-200 outline-none"
          >
            {SCENE_QUALITIES.map((option) => (
              <option key={option} value={option} className="bg-void-900">
                {option}
              </option>
            ))}
          </select>
          <span
            className="font-mono text-[11px] text-slate-300"
            title="Frames per second (live)"
          >
            {fps > 0 ? `${fps} fps` : "— fps"}
          </span>
        </div>
      </div>
      <div className="flex flex-col items-center gap-1">
        {tooltip !== null ? (
          <p
            role="status"
            className="rounded border border-cyan-300/30 bg-void-950/90 px-2 py-0.5 font-mono text-[11px] text-cyan-200"
          >
            {`${tooltip.label} · ${describeNode(tooltip)}`}
          </p>
        ) : null}
        <p className="text-center text-[11px] text-slate-500">
          auto-orbit engaged · select a node for inspector details
        </p>
      </div>
    </div>
  );
}
