import type { AgentDescriptor, TaskRecord } from "@talos/contracts";

/** Left-rail navigation target. */
export interface NavItem {
  label: string;
  to: string;
  hint: string;
}

/** A feed entry for dashboard / audit surfaces. */
export interface ActivityEvent {
  id: string;
  at: string;
  actor: string;
  message: string;
  tone: "info" | "success" | "warning" | "error";
}

/** Entity selected in the right inspector. */
export interface SelectedEntity {
  kind: "agent" | "task" | "evidence";
  id: string;
  label: string;
  detail: string;
}

export type ConnectionState = "connected" | "connecting" | "disconnected";

export interface MockSnapshot {
  agents: AgentDescriptor[];
  tasks: TaskRecord[];
  activity: ActivityEvent[];
}
