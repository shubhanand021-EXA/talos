import { useMemo } from "react";
import * as THREE from "three";
import { useSceneSettings } from "./scene-settings.js";

/** Deterministic starfield: seeded LCG, no Math.random, no textures. */
function seededPositions(count: number): Float32Array {
  const positions = new Float32Array(count * 3);
  let seed = 0x2f6e2b1;
  const next = () => {
    seed = (seed * 1664525 + 1013904223) >>> 0;
    return seed / 0xffffffff;
  };
  for (let i = 0; i < count; i += 1) {
    const r = 14 + next() * 18;
    const theta = next() * Math.PI * 2;
    const y = (next() - 0.5) * 16;
    positions[i * 3] = Math.cos(theta) * r;
    positions[i * 3 + 1] = y;
    positions[i * 3 + 2] = Math.sin(theta) * r;
  }
  return positions;
}

/** Faint grid + starfield backdrop. Static geometry, zero per-frame cost. */
export function Backdrop() {
  const { quality } = useSceneSettings();
  const count = quality === "high" ? 800 : quality === "medium" ? 400 : 150;

  const geometry = useMemo(() => {
    const geo = new THREE.BufferGeometry();
    geo.setAttribute(
      "position",
      new THREE.BufferAttribute(seededPositions(count), 3),
    );
    return geo;
  }, [count]);
  const material = useMemo(
    () =>
      new THREE.PointsMaterial({
        color: "#33506b",
        size: 0.06,
        transparent: true,
        opacity: 0.8,
        depthWrite: false,
      }),
    [],
  );

  return (
    <group>
      <gridHelper
        args={[40, 40, "#164e63", "#101828"]}
        position={[0, -2.4, 0]}
      />
      <points geometry={geometry} material={material} />
    </group>
  );
}
