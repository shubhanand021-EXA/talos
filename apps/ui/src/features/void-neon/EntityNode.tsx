import { useMemo, useRef, useState } from "react";
import { useFrame, type ThreeEvent } from "@react-three/fiber";
import * as THREE from "three";
import type { SceneNode } from "@talos/contracts";
import { useSceneSettings } from "./scene-settings.js";
import {
  STATUS_COLORS,
  fidelityIntensity,
  type TaskFidelity,
} from "./topology.js";
import type { TopologyStatus } from "./types.js";

interface EntityNodeProps {
  node: SceneNode;
  lifecycle: TopologyStatus;
  selected: boolean;
  /** Focus mode: non-neighborhood nodes render dimmed. */
  dimmed: boolean;
  /** Recent activity or replay highlight: brighter emissive pulse. */
  highlighted: boolean;
  /** Execution fidelity: mock dims, verified glows + checkmark. */
  fidelity?: TaskFidelity;
  onSelect: (node: SceneNode) => void;
  onHover: (node: SceneNode | null) => void;
}

function positionOf(node: SceneNode): [number, number, number] {
  const p = node.position;
  return [p?.x ?? 0, p?.y ?? 0, p?.z ?? 0];
}

/** Geometry per node kind — distinct silhouettes, shared materials. */
function NodeGeometry({ kind }: { kind: SceneNode["kind"] }) {
  switch (kind) {
    case "task":
      return <boxGeometry args={[0.55, 0.55, 0.55]} />;
    case "tool":
      return <tetrahedronGeometry args={[0.55]} />;
    case "browser":
      return <torusGeometry args={[0.42, 0.13, 12, 24]} />;
    case "computer-use":
      return <coneGeometry args={[0.45, 0.7, 4]} />;
    case "policy":
      return <octahedronGeometry args={[0.5, 0]} />;
    case "evidence":
      return <icosahedronGeometry args={[0.35, 0]} />;
    case "audit":
      return <dodecahedronGeometry args={[0.4, 0]} />;
    default:
      return <octahedronGeometry args={[0.42, 0]} />;
  }
}

/**
 * One non-agent scene node: task, tool, browser, computer-use, policy gate,
 * ledger, or audit sentinel. Click to select, hover for tooltip, dims outside
 * the focus neighborhood, glows on recent activity. No per-frame allocation.
 */
export function EntityNode({
  node,
  lifecycle,
  selected,
  dimmed,
  highlighted,
  fidelity = "mock",
  onSelect,
  onHover,
}: EntityNodeProps) {
  const { reduceMotion } = useSceneSettings();
  const mesh = useRef<THREE.Mesh>(null);
  const [hovered, setHovered] = useState(false);
  const color = STATUS_COLORS[lifecycle];

  const material = useMemo(
    () =>
      new THREE.MeshStandardMaterial({
        color: "#0a0c14",
        emissive: color,
        emissiveIntensity:
          (dimmed ? 0.25 : highlighted || selected ? 2.2 : 1.1) *
          fidelityIntensity(fidelity),
        roughness: 0.3,
        metalness: 0.2,
        transparent: dimmed,
        opacity: dimmed ? 0.45 : 1,
      }),
    [color, dimmed, highlighted, selected, fidelity],
  );

  useFrame((state) => {
    if (reduceMotion || mesh.current === null) {
      return;
    }
    mesh.current.rotation.y = state.clock.elapsedTime * 0.4;
    const pulse =
      highlighted || selected
        ? 1 + Math.sin(state.clock.elapsedTime * 3) * 0.08
        : 1;
    mesh.current.scale.setScalar((selected ? 1.25 : 1) * pulse);
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
          onHover(node);
          document.body.style.cursor = "pointer";
        }}
        onPointerOut={() => {
          setHovered(false);
          onHover(null);
          document.body.style.cursor = "auto";
        }}
      >
        <NodeGeometry kind={node.kind} />
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
      {fidelity === "verified" ? (
        <mesh scale={1.35}>
          <sphereGeometry args={[0.42, 16, 16]} />
          <meshBasicMaterial
            color="#34d399"
            wireframe
            transparent
            opacity={0.5}
          />
        </mesh>
      ) : null}
    </group>
  );
}
