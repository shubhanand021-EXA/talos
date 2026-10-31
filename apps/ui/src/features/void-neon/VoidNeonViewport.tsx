import { useCallback, useEffect, useMemo, useState } from "react";
import {
  ToolIdSchema,
  type SceneNode,
  type SceneNodeKind,
} from "@talos/contracts";
import { EmptyState } from "../../design-system/EmptyState.js";
import { getMockAgents, getMockTasks } from "../../stores/mockStore.js";
import { useAppStore } from "../../stores/appStore.js";
import { useUiStore } from "../../stores/uiStore.js";
import { SceneErrorBoundary } from "./SceneErrorBoundary.js";
import { SceneHud } from "./SceneHud.js";
import { SceneSettingsProvider } from "./SceneSettingsProvider.js";
import { ViewportControls, type ReplayEvent } from "./ViewportControls.js";
import { VoidNeonCanvas } from "./VoidNeonCanvas.js";
import {
  applyKindFilter,
  buildLiveTopology,
  countFeedSignals,
  describeNode,
  KIND_FILTER_GROUPS,
  recentTaskIds,
  selectKindFor,
  toTaskTopologyStatus,
  toTopologyStatus,
} from "./topology.js";
import { isWebGLAvailable } from "./webgl.js";

/** All filterable kinds visible by default. */
const ALL_FILTER_KINDS: ReadonlySet<SceneNodeKind> = new Set(
  KIND_FILTER_GROUPS.map(({ kind }) => kind),
);

/** Recent-activity glow window (60s of feed history). */
const RECENT_WINDOW_MS = 60_000;

/**
 * Dashboard viewport: live operational topology (tasks, tools, browser,
 * computer-use, policy gate, ledger, audit sentinel), HUD overlay, kind
 * filters, focus mode, replay slider, selection wired into the inspector.
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
  const liveTasks = useAppStore((state) => state.liveTasks);
  const liveFeed = useAppStore((state) => state.liveFeed);
  const computerSafety = useAppStore((state) => state.computerSafety);
  const lastEvaluation = useAppStore((state) => state.lastEvaluation);
  const evidenceVersion = useAppStore((state) => state.evidenceVersion);
  const ledgerHealthy = useAppStore(
    (state) => state.systemStatus.evidenceLedgerHealthy,
  );
  const setViewportFps = useUiStore((state) => state.setViewportFps);
  const [fps, setFps] = useState(0);
  const [webgl] = useState(isWebGLAvailable);
  const [hoveredId, setHoveredId] = useState<string | null>(null);
  const [visibleKinds, setVisibleKinds] =
    useState<ReadonlySet<SceneNodeKind>>(ALL_FILTER_KINDS);
  const [focusMode, setFocusMode] = useState(false);
  const [replayIndex, setReplayIndex] = useState<number | null>(null);
  // Wall clock for the recent-activity window (5s tick, no render impurity).
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    const timer = setInterval(() => {
      setNow(Date.now());
    }, 5_000);
    return () => {
      clearInterval(timer);
    };
  }, []);

  const selectEntity = useUiStore((state) => state.selectEntity);
  const selectedEntity = useUiStore((state) => state.selectedEntity);

  const handleFps = useCallback(
    (value: number) => {
      setFps(value);
      setViewportFps(value);
    },
    [setViewportFps],
  );

  const agents = useMemo(
    () => [...getMockAgents(), ...liveAgents],
    [liveAgents],
  );
  const tasks = useMemo(() => {
    const mock = getMockTasks().map((task) => ({
      id: task.id,
      title: task.title,
      status: task.status,
    }));
    const live = liveTasks.map((task) => ({
      id: task.id,
      title: task.title,
      status: task.status,
    }));
    const seen = new Set(live.map((task) => task.id));
    return [...live, ...mock.filter((task) => !seen.has(task.id))];
  }, [liveTasks]);

  const fullTopology = useMemo(
    () =>
      buildLiveTopology({
        agents,
        tasks,
        toolIds: [...ToolIdSchema.options],
        computerStopped: computerSafety?.emergencyStopped === true,
        policyDecision: lastEvaluation?.decision ?? null,
        evidenceCount: evidenceVersion,
      }),
    [agents, tasks, computerSafety, lastEvaluation, evidenceVersion],
  );

  const topology = useMemo(
    () => applyKindFilter(fullTopology, visibleKinds),
    [fullTopology, visibleKinds],
  );

  const lifecycleById = useMemo(() => {
    const byId = new Map<string, ReturnType<typeof toTopologyStatus>>();
    for (const node of topology.nodes) {
      const meta = node.metadata as { lifecycle?: string } | undefined;
      const lifecycle = meta?.lifecycle;
      if (node.kind === "system") {
        byId.set(
          node.id,
          computerSafety?.emergencyStopped === true ? "stopped" : "running",
        );
      } else if (
        lifecycle === "idle" ||
        lifecycle === "running" ||
        lifecycle === "queued" ||
        lifecycle === "waiting_policy" ||
        lifecycle === "denied" ||
        lifecycle === "failed" ||
        lifecycle === "completed" ||
        lifecycle === "canceled" ||
        lifecycle === "stopped"
      ) {
        byId.set(node.id, lifecycle);
      } else {
        byId.set(node.id, "idle");
      }
    }
    return byId;
  }, [topology, computerSafety]);

  // Focus neighborhood: selection + directly linked nodes.
  const focusNeighbors = useMemo(() => {
    if (!focusMode || selectedEntity === null) {
      return null;
    }
    const neighbors = new Set<string>([selectedEntity.id]);
    for (const edge of topology.edges) {
      if (edge.source === selectedEntity.id) {
        neighbors.add(edge.target);
      } else if (edge.target === selectedEntity.id) {
        neighbors.add(edge.source);
      }
    }
    return neighbors;
  }, [focusMode, selectedEntity, topology]);

  // Recent activity glow: feed task ids inside the window + linked agents.
  const recentIds = useMemo(() => {
    const recent = recentTaskIds(liveFeed, now, RECENT_WINDOW_MS);
    const ids = new Set<string>(recent);
    for (const node of topology.nodes) {
      const taskId = (node.metadata as { taskId?: string } | undefined)?.taskId;
      if (taskId !== undefined && recent.has(taskId)) {
        ids.add(node.id);
      }
    }
    if (replayIndex !== null) {
      const event = liveFeed
        .filter((entry) => entry.taskId !== undefined)
        .slice(-20)[replayIndex];
      if (event?.taskId !== undefined) {
        ids.add(event.taskId);
      }
    }
    return ids;
  }, [liveFeed, now, replayIndex, topology]);

  const replayEvents: ReplayEvent[] = useMemo(
    () =>
      liveFeed
        .filter((entry) => entry.taskId !== undefined)
        .slice(-20)
        .map((entry) => ({
          id: entry.id,
          label: `${entry.actor}: ${entry.message}`,
          taskId: entry.taskId,
        })),
    [liveFeed],
  );

  const handleSelectNode = useCallback(
    (node: SceneNode) => {
      selectEntity({
        kind: selectKindFor(node),
        id: node.id,
        label: node.label,
        detail: describeNode(node),
      });
    },
    [selectEntity],
  );

  const handleHoverNode = useCallback((node: SceneNode | null) => {
    setHoveredId(node?.id ?? null);
  }, []);

  const toggleKind = useCallback((kind: SceneNodeKind) => {
    setVisibleKinds((current) => {
      const next = new Set(current);
      if (next.has(kind)) {
        next.delete(kind);
      } else {
        next.add(kind);
      }
      return next;
    });
  }, []);

  const selectedNode = useMemo(
    () =>
      selectedEntity === null
        ? null
        : (topology.nodes.find((node) => node.id === selectedEntity.id) ??
          null),
    [topology, selectedEntity],
  );

  const hoveredNode = useMemo(
    () =>
      hoveredId === null
        ? null
        : (topology.nodes.find((node) => node.id === hoveredId) ?? null),
    [topology, hoveredId],
  );

  const counts = useMemo(() => {
    const tasksActive = tasks.filter((task) => {
      const lifecycle = toTaskTopologyStatus(task.status);
      return lifecycle === "running" || lifecycle === "queued";
    }).length;
    const waitingPolicy = tasks.filter(
      (task) => toTaskTopologyStatus(task.status) === "waiting_policy",
    ).length;
    const signals = countFeedSignals(liveFeed);
    return {
      tasksActive,
      waitingPolicy,
      browserEvents: signals.browser,
      computerEvents: signals.computer,
    };
  }, [tasks, liveFeed]);

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
          lifecycleById={lifecycleById}
          selectedId={selectedNode?.id ?? null}
          focusNeighbors={focusNeighbors}
          recentIds={recentIds}
          onSelectNode={handleSelectNode}
          onHoverNode={handleHoverNode}
          onFps={handleFps}
        />
        <SceneHud
          activeCount={topology.nodes.length}
          selected={selectedNode}
          hovered={hoveredNode}
          fps={fps}
          counts={counts}
          safety={
            computerSafety === null
              ? null
              : {
                  stopped: computerSafety.emergencyStopped,
                  enabled: computerSafety.enabled,
                }
          }
          ledgerHealthy={ledgerHealthy}
          evidenceVersion={evidenceVersion}
        />
        <ViewportControls
          visibleKinds={visibleKinds}
          onToggleKind={toggleKind}
          focusMode={focusMode}
          onToggleFocus={() => {
            setFocusMode((current) => !current);
          }}
          hasSelection={selectedEntity !== null}
          replayEvents={replayEvents}
          replayIndex={replayIndex}
          onReplayChange={setReplayIndex}
        />
      </SceneErrorBoundary>
    </div>
  );
}
