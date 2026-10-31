import { StatusPill } from "../../design-system/StatusPill.js";
import type { SceneNode } from "@talos/contracts";
import { useSceneSettings } from "./scene-settings.js";
import { SCENE_QUALITIES, type SceneQuality } from "./types.js";

interface SceneHudProps {
  activeCount: number;
  selected: SceneNode | null;
  fps: number;
}

/** Glass HUD overlay: nodes, selection, quality, FPS. DOM only — always readable. */
export function SceneHud({ activeCount, selected, fps }: SceneHudProps) {
  const { quality, setQuality } = useSceneSettings();

  return (
    <div className="pointer-events-none absolute inset-0 flex flex-col justify-between p-3">
      <div className="flex items-start justify-between gap-2">
        <div className="pointer-events-auto flex items-center gap-2">
          <StatusPill tone="info">{`${activeCount} nodes live`}</StatusPill>
          {selected !== null ? (
            <StatusPill tone="ok">{selected.label}</StatusPill>
          ) : (
            <StatusPill tone="neutral">no selection</StatusPill>
          )}
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
      <p className="text-center text-[11px] text-slate-500">
        auto-orbit engaged · select a node for inspector details
      </p>
    </div>
  );
}
