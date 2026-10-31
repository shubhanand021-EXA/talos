import { useMemo, useRef } from "react";
import { useFrame } from "@react-three/fiber";
import * as THREE from "three";
import { useSceneSettings } from "./scene-settings.js";
import { edgeColorFor } from "./topology.js";
import type { Topology } from "./types.js";

function pointOf(topology: Topology, id: string): THREE.Vector3 {
  const node = topology.nodes.find((n) => n.id === id);
  const p = node?.position;
  return new THREE.Vector3(p?.x ?? 0, p?.y ?? 0, p?.z ?? 0);
}

/**
 * Core-to-agent links: static additive lines plus light pulses traveling
 * from core to edge. Pure function of topology; no per-frame allocation.
 */
export function LinkFlow({ topology }: { topology: Topology }) {
  const { reduceMotion } = useSceneSettings();
  const pulses = useRef<THREE.Mesh[]>([]);

  const { lineGeometry, lineMaterial, lanes } = useMemo(() => {
    const positions: number[] = [];
    const colors: number[] = [];
    const lanes = topology.edges.map((edge) => {
      const from = pointOf(topology, edge.source);
      const to = pointOf(topology, edge.target);
      positions.push(from.x, from.y, from.z, to.x, to.y, to.z);
      const color = new THREE.Color(edgeColorFor(topology, edge));
      colors.push(color.r, color.g, color.b, color.r, color.g, color.b);
      return { from, to, color };
    });
    const lineGeometry = new THREE.BufferGeometry();
    lineGeometry.setAttribute(
      "position",
      new THREE.Float32BufferAttribute(positions, 3),
    );
    lineGeometry.setAttribute(
      "color",
      new THREE.Float32BufferAttribute(colors, 3),
    );
    const lineMaterial = new THREE.LineBasicMaterial({
      vertexColors: true,
      transparent: true,
      opacity: 0.5,
      blending: THREE.AdditiveBlending,
      depthWrite: false,
    });
    return { lineGeometry, lineMaterial, lanes };
  }, [topology]);

  const pulseMaterial = useMemo(
    () =>
      new THREE.MeshBasicMaterial({
        color: "#a5f3fc",
        transparent: true,
        opacity: 0.9,
        blending: THREE.AdditiveBlending,
        depthWrite: false,
      }),
    [],
  );

  useFrame((state) => {
    const t = reduceMotion ? 0.5 : (state.clock.elapsedTime * 0.25) % 1;
    pulses.current.forEach((pulse, index) => {
      const lane = lanes[index];
      if (lane === undefined) {
        return;
      }
      pulse.position.lerpVectors(lane.from, lane.to, (t + index * 0.13) % 1);
    });
  });

  return (
    <group>
      <lineSegments geometry={lineGeometry} material={lineMaterial} />
      {lanes.map((lane, index) => (
        <mesh
          key={`${lane.from.x}-${lane.to.x}-${index}`}
          ref={(mesh) => {
            pulses.current[index] = mesh as THREE.Mesh;
          }}
          material={pulseMaterial}
        >
          <sphereGeometry args={[0.07, 12, 12]} />
        </mesh>
      ))}
    </group>
  );
}
