import { createContext, useContext } from "react";
import type { SceneQuality } from "./types.js";

export interface SceneSettings {
  quality: SceneQuality;
  setQuality: (quality: SceneQuality) => void;
  reduceMotion: boolean;
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
