import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { SceneSettingsContext } from "./scene-settings.js";
import { nextLiteMode, type LiteOverride } from "./lite-mode.js";
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
  const [liteMode, setLiteMode] = useState(false);
  const [liteOverride, setLiteOverride] = useState<LiteOverride>("auto");
  const samples = useRef<number[]>([]);

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

  const reportFps = useCallback(
    (fps: number) => {
      samples.current = [...samples.current.slice(-11), fps];
      const decision = nextLiteMode({
        fpsSamples: samples.current,
        current: liteMode,
        override: liteOverride,
      });
      if (decision.lite !== liteMode) {
        setLiteMode(decision.lite);
      }
    },
    [liteMode, liteOverride],
  );

  const value = useMemo(
    () => ({
      quality,
      setQuality,
      reduceMotion,
      liteMode,
      liteOverride,
      setLiteOverride,
      reportFps,
    }),
    [quality, reduceMotion, liteMode, liteOverride, reportFps],
  );
  return (
    <SceneSettingsContext.Provider value={value}>
      {children}
    </SceneSettingsContext.Provider>
  );
}
