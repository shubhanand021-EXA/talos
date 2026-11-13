import { createContext, useContext } from "react";
import type { LiteOverride } from "./lite-mode.js";
import type { SceneQuality } from "./types.js";

export interface SceneSettings {
  quality: SceneQuality;
  setQuality: (quality: SceneQuality) => void;
  reduceMotion: boolean;
  /** Auto lite-mode engaged (effects reduced, identity unchanged). */
  liteMode: boolean;
  /** Manual override: auto follows the governor, lite/full win outright. */
  liteOverride: LiteOverride;
  setLiteOverride: (override: LiteOverride) => void;
  /** Feed one throttled fps sample into the governor. */
  reportFps: (fps: number) => void;
}

export const SceneSettingsContext = createContext<SceneSettings | null>(null);

/** Quality + motion preference for the void-neon scene. */
export function useSceneSettings(): SceneSettings {
  const settings = useContext(SceneSettingsContext);
  if (settings === null) {
    throw new Error(
      "useSceneSettings must be used inside SceneSettingsProvider",
    );
  }
  return settings;
}
