import { useEffect, useRef } from "react";
import { useFrame } from "@react-three/fiber";

/**
 * R3F frame-rate reporter. Measures real frames and publishes a throttled
 * value (~2×/s) through `onFps`; renders nothing.
 */
export function FpsMeter({ onFps }: { onFps: (fps: number) => void }) {
  const frames = useRef(0);
  const last = useRef(0);
  const callback = useRef(onFps);

  useEffect(() => {
    callback.current = onFps;
  }, [onFps]);

  useFrame((state) => {
    frames.current += 1;
    const now = state.clock.elapsedTime;
    if (now - last.current >= 0.5) {
      const value = Math.round(frames.current / (now - last.current));
      frames.current = 0;
      last.current = now;
      callback.current(value);
    }
  });
  return null;
}
