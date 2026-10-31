import { useEffect, useMemo, useState, type ReactNode } from "react";
import { SceneSettingsContext } from "./scene-settings.js";
import type { SceneQuality } from "./types.js";

function prefersReducedMotion(): boolean {
  if (
    typeof window === "undefined" ||
    typeof window.matchMedia !== "function"
  ) {
    return false;
  }
  return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

export function SceneSettingsProvider({ children }: { children: ReactNode }) {
  const [quality, setQuality] = useState<SceneQuality>("medium");
  const [reduceMotion, setReduceMotion] =
    useState<boolean>(prefersReducedMotion);

  useEffect(() => {
    if (
      typeof window === "undefined" ||
      typeof window.matchMedia !== "function"
    ) {
      return;
    }
    const query = window.matchMedia("(prefers-reduced-motion: reduce)");
    const onChange = () => {
      setReduceMotion(query.matches);
    };
    query.addEventListener("change", onChange);
    return () => {
      query.removeEventListener("change", onChange);
    };
  }, []);

  const value = useMemo(
    () => ({ quality, setQuality, reduceMotion }),
    [quality, reduceMotion],
  );
  return (
    <SceneSettingsContext.Provider value={value}>
      {children}
    </SceneSettingsContext.Provider>
  );
}
