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
  /** Owning task, when the event belongs to a queue task (enables filtering). */
  taskId?: string;
}

/** Entity selected in the right inspector. */
export interface SelectedEntity {
  kind: "agent" | "task" | "evidence" | "system";
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
