import { useMemo, useRef } from "react";
import { useFrame } from "@react-three/fiber";
import * as THREE from "three";
import { useSceneSettings } from "./scene-settings.js";

/** Central Talos core: emissive icosahedron with slow cinematic rotation. */
export function TalosCore() {
  const { reduceMotion } = useSceneSettings();
  const outer = useRef<THREE.Mesh>(null);
  const inner = useRef<THREE.Mesh>(null);

  useFrame((state) => {
    if (reduceMotion) {
      return;
    }
    const t = state.clock.elapsedTime;
    if (outer.current !== null) {
      outer.current.rotation.y = t * 0.12;
      outer.current.rotation.x = Math.sin(t * 0.2) * 0.15;
    }
    if (inner.current !== null) {
      const s = 1 + Math.sin(t * 1.4) * 0.06;
      inner.current.scale.setScalar(s);
    }
  });

  const innerMaterial = useMemo(
    () =>
      new THREE.MeshStandardMaterial({
        color: "#062a33",
        emissive: "#22d3ee",
        emissiveIntensity: 1.6,
        roughness: 0.25,
        metalness: 0.1,
      }),
    [],
  );
  const outerMaterial = useMemo(
    () =>
      new THREE.MeshStandardMaterial({
        color: "#0a0c14",
        emissive: "#8b5cf6",
        emissiveIntensity: 0.5,
        wireframe: true,
        transparent: true,
        opacity: 0.55,
      }),
    [],
  );

  return (
    <group>
      <mesh ref={inner} material={innerMaterial}>
        <icosahedronGeometry args={[0.9, 1]} />
      </mesh>
      <mesh ref={outer} scale={1.55} material={outerMaterial}>
        <icosahedronGeometry args={[0.9, 1]} />
      </mesh>
      <pointLight color="#22d3ee" intensity={18} distance={14} decay={2} />
    </group>
  );
}
