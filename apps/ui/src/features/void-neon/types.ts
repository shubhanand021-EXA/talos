import type { SceneEdge, SceneNode } from "@talos/contracts";

/** Render quality tier. Controls pixel ratio, bloom, and particle density. */
export type SceneQuality = "low" | "medium" | "high";

export const SCENE_QUALITIES: SceneQuality[] = ["low", "medium", "high"];

/** Viewport topology: nodes plus core-anchored edges. */
export interface Topology {
  nodes: SceneNode[];
  edges: SceneEdge[];
  coreId: string;
}

/** Node status values the scene visualizes (task + agent lifecycles). */
export type TopologyStatus =
  | "idle"
  | "running"
  | "queued"
  | "waiting_policy"
  | "denied"
  | "failed"
  | "completed"
  | "canceled"
  | "stopped";
