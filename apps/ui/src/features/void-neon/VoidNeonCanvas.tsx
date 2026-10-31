import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { Canvas, useFrame } from "@react-three/fiber";
import { EffectComposer, Bloom } from "@react-three/postprocessing";
import * as THREE from "three";
import { AgentNode } from "./AgentNode.js";
import { Backdrop } from "./Backdrop.js";
import { EntityNode } from "./EntityNode.js";
import { FpsMeter } from "./FpsMeter.js";
import { LinkFlow } from "./LinkFlow.js";
import { useSceneSettings } from "./scene-settings.js";
import { TalosCore } from "./TalosCore.js";
import type { SceneNode } from "@talos/contracts";
import type { Topology, TopologyStatus } from "./types.js";

/** Slow cinematic orbit rig. Static when reduced motion is requested. */
function OrbitRig({ children }: { children: ReactNode }) {
  const { reduceMotion } = useSceneSettings();
  const rig = useRef<THREE.Group>(null);

  useFrame((state) => {
    if (reduceMotion || rig.current === null) {
      return;
    }
    rig.current.rotation.y = state.clock.elapsedTime * 0.05;
  });

  return <group ref={rig}>{children}</group>;
}

interface VoidNeonSceneProps {
  topology: Topology;
  lifecycleById: Map<string, TopologyStatus>;
  selectedId: string | null;
  /** Focus neighborhood: null disables focus dimming. */
  focusNeighbors: ReadonlySet<string> | null;
  /** Recently active node ids: glow pulse. */
  recentIds: ReadonlySet<string>;
  onSelectNode: (node: SceneNode) => void;
  onHoverNode: (node: SceneNode | null) => void;
  onFps: (fps: number) => void;
}

function VoidNeonScene({
  topology,
  lifecycleById,
  selectedId,
  focusNeighbors,
  recentIds,
  onSelectNode,
  onHoverNode,
  onFps,
}: VoidNeonSceneProps) {
  const { quality, reduceMotion } = useSceneSettings();
  const agents = useMemo(
    () => topology.nodes.filter((node) => node.kind === "agent"),
    [topology],
  );
  const entities = useMemo(
    () =>
      topology.nodes.filter(
        (node) => node.kind !== "agent" && node.kind !== "system",
      ),
    [topology],
  );

  return (
    <>
      <color attach="background" args={["#05060a"]} />
      <fog attach="fog" args={["#05060a", 12, 30]} />
      <ambientLight intensity={0.5} />
      <OrbitRig>
        <Backdrop />
        <TalosCore />
        {agents.map((node) => (
          <AgentNode
            key={node.id}
            node={node}
            lifecycle={lifecycleById.get(node.id) ?? "idle"}
            selected={selectedId === node.id}
            dimmed={focusNeighbors !== null && !focusNeighbors.has(node.id)}
            highlighted={recentIds.has(node.id)}
            onSelect={onSelectNode}
            onHover={onHoverNode}
          />
        ))}
        {entities.map((node) => (
          <EntityNode
            key={node.id}
            node={node}
            lifecycle={lifecycleById.get(node.id) ?? "idle"}
            selected={selectedId === node.id}
            dimmed={focusNeighbors !== null && !focusNeighbors.has(node.id)}
            highlighted={recentIds.has(node.id)}
            onSelect={onSelectNode}
            onHover={onHoverNode}
          />
        ))}
        <LinkFlow topology={topology} />
      </OrbitRig>
      <FpsMeter onFps={onFps} />
      {quality !== "low" && !reduceMotion ? (
        <EffectComposer multisampling={quality === "high" ? 4 : 0}>
          <Bloom
            intensity={quality === "high" ? 1.1 : 0.7}
            luminanceThreshold={0.15}
            luminanceSmoothing={0.2}
            mipmapBlur
          />
        </EffectComposer>
      ) : null}
    </>
  );
}

interface VoidNeonCanvasProps {
  topology: Topology;
  lifecycleById: Map<string, TopologyStatus>;
  selectedId: string | null;
  focusNeighbors: ReadonlySet<string> | null;
  recentIds: ReadonlySet<string>;
  onSelectNode: (node: SceneNode) => void;
  onHoverNode: (node: SceneNode | null) => void;
  onFps: (fps: number) => void;
}

/**
 * WebGL canvas with deterministic DPR. Rendering pauses while the tab is
 * hidden to save GPU; nothing WebGL-related mounts when support is absent.
 */
export function VoidNeonCanvas({
  topology,
  lifecycleById,
  selectedId,
  focusNeighbors,
  recentIds,
  onSelectNode,
  onHoverNode,
  onFps,
}: VoidNeonCanvasProps) {
  const { quality } = useSceneSettings();
  const [hidden, setHidden] = useState(
    () => typeof document !== "undefined" && document.hidden,
  );

  useEffect(() => {
    const onVisibility = () => {
      setHidden(document.hidden);
    };
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, []);

  return (
    <Canvas
      dpr={
        quality === "high" ? [1, 2] : quality === "medium" ? [1, 1.5] : [1, 1]
      }
      gl={{ antialias: quality !== "low", powerPreference: "high-performance" }}
      camera={{ position: [0, 4.4, 10.5], fov: 50 }}
      frameloop={hidden ? "never" : "always"}
      role="img"
      aria-label="Void Neon spatial topology: Talos core with linked agent nodes"
    >
      <VoidNeonScene
        topology={topology}
        lifecycleById={lifecycleById}
        selectedId={selectedId}
        focusNeighbors={focusNeighbors}
        recentIds={recentIds}
        onSelectNode={onSelectNode}
        onHoverNode={onHoverNode}
        onFps={onFps}
      />
    </Canvas>
  );
}
