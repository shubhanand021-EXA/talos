import { useMemo, useRef, useState } from "react";
import { useFrame, type ThreeEvent } from "@react-three/fiber";
import * as THREE from "three";
import type { SceneNode } from "@talos/contracts";
import { useSceneSettings } from "./scene-settings.js";
import { STATUS_COLORS } from "./topology.js";
import type { TopologyStatus } from "./types.js";

interface AgentNodeProps {
  node: SceneNode;
  lifecycle: TopologyStatus;
  selected: boolean;
  onSelect: (node: SceneNode) => void;
}

function positionOf(node: SceneNode): [number, number, number] {
  const p = node.position;
  return [p?.x ?? 0, p?.y ?? 0, p?.z ?? 0];
}

/** One agent satellite: emissive octahedron, status-colored, clickable. */
export function AgentNode({
  node,
  lifecycle,
  selected,
  onSelect,
}: AgentNodeProps) {
  const { reduceMotion } = useSceneSettings();
  const mesh = useRef<THREE.Mesh>(null);
  const [hovered, setHovered] = useState(false);
  const color = STATUS_COLORS[lifecycle];

  const material = useMemo(
    () =>
      new THREE.MeshStandardMaterial({
        color: "#0a0c14",
        emissive: color,
        emissiveIntensity: selected ? 2.2 : 1.1,
        roughness: 0.3,
        metalness: 0.2,
      }),
    [color, selected],
  );

  useFrame((state) => {
    if (reduceMotion || mesh.current === null) {
      return;
    }
    mesh.current.rotation.y = state.clock.elapsedTime * 0.5;
    const s =
      (selected ? 1.25 : 1) + Math.sin(state.clock.elapsedTime * 2) * 0.05;
    mesh.current.scale.setScalar(s);
  });

  const handleClick = (event: ThreeEvent<MouseEvent>) => {
    event.stopPropagation();
    onSelect(node);
  };

  return (
    <group position={positionOf(node)}>
      <mesh
        ref={mesh}
        material={material}
        scale={selected ? 1.25 : 1}
        onClick={handleClick}
        onPointerOver={(event) => {
          event.stopPropagation();
          setHovered(true);
          document.body.style.cursor = "pointer";
        }}
        onPointerOut={() => {
          setHovered(false);
          document.body.style.cursor = "auto";
        }}
      >
        <octahedronGeometry args={[0.42, 0]} />
      </mesh>
      {selected || hovered ? (
        <mesh scale={1.9}>
          <sphereGeometry args={[0.42, 16, 16]} />
          <meshBasicMaterial
            color={color}
            wireframe
            transparent
            opacity={0.35}
          />
        </mesh>
      ) : null}
    </group>
  );
}
