import { useCallback, useMemo, useState } from "react";
import type { SceneNode } from "@talos/contracts";
import { EmptyState } from "../../design-system/EmptyState.js";
import { getMockAgents } from "../../stores/mockStore.js";
import { useAppStore } from "../../stores/appStore.js";
import { useUiStore } from "../../stores/uiStore.js";
import { SceneErrorBoundary } from "./SceneErrorBoundary.js";
import { SceneHud } from "./SceneHud.js";
import { SceneSettingsProvider } from "./SceneSettingsProvider.js";
import { VoidNeonCanvas } from "./VoidNeonCanvas.js";
import { buildTopology, toTopologyStatus } from "./topology.js";
import { isWebGLAvailable } from "./webgl.js";

function describeNode(node: SceneNode): string {
  const meta = node.metadata as
    { role?: string; lifecycle?: string } | undefined;
  const role = meta?.role ?? node.kind;
  const lifecycle = meta?.lifecycle;
  return lifecycle === undefined ? role : `${role} · ${lifecycle}`;
}

/**
 * Dashboard viewport: deterministic topology from mock agents, HUD overlay,
 * selection wired into the right inspector. Live events replace the mock
 * source here in later batches.
 */
export function VoidNeonViewport() {
  return (
    <SceneSettingsProvider>
      <ViewportInner />
    </SceneSettingsProvider>
  );
}

function ViewportInner() {
  const liveAgents = useAppStore((state) => state.liveAgents);
  const agents = useMemo(
    () => [...getMockAgents(), ...liveAgents],
    [liveAgents],
  );
  const selectEntity = useUiStore((state) => state.selectEntity);
  const selectedEntity = useUiStore((state) => state.selectedEntity);
  const [fps, setFps] = useState(0);
  const [webgl] = useState(isWebGLAvailable);

  const topology = useMemo(() => buildTopology(agents), [agents]);
  const statusById = useMemo(
    () =>
      new Map(
        agents.map((agent) => [
          agent.id as string,
          toTopologyStatus(agent.status),
        ]),
      ),
    [agents],
  );

  const handleSelectNode = useCallback(
    (node: SceneNode) => {
      selectEntity({
        kind: node.kind === "agent" ? "agent" : "system",
        id: node.id,
        label: node.label,
        detail: describeNode(node),
      });
    },
    [selectEntity],
  );

  const selectedNode = useMemo(
    () =>
      selectedEntity === null
        ? null
        : (topology.nodes.find((node) => node.id === selectedEntity.id) ??
          null),
    [topology, selectedEntity],
  );

  if (!webgl) {
    return (
      <div className="flex h-[420px] items-center justify-center">
        <EmptyState
          title="Viewport unavailable"
          message="WebGL could not be initialized on this device, so the 3D topology is parked. Agent data above remains fully usable."
        />
      </div>
    );
  }

  return (
    <div className="relative h-[420px] overflow-hidden rounded-panel">
      <SceneErrorBoundary>
        <VoidNeonCanvas
          topology={topology}
          statusById={statusById}
          selectedId={selectedNode?.id ?? null}
          onSelectNode={handleSelectNode}
          onFps={setFps}
        />
        <SceneHud
          activeCount={topology.nodes.length}
          selected={selectedNode}
          fps={fps}
        />
      </SceneErrorBoundary>
    </div>
  );
}
