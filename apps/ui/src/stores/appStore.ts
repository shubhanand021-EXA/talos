import type {
  AgentDescriptor,
  ContextCompactionReceipt,
  PolicyEvaluation,
  SystemStatus,
  TaskQueueEntry,
} from "@talos/contracts";
import { create } from "zustand";
import type { ActivityEvent, ConnectionState } from "../types/index.js";
import type { ComputerSafetyStatus } from "../bridge/index.js";

export interface AppInfo {
  name: string;
  version: string;
  environment: "local" | "staging" | "production";
}

interface AppState {
  appInfo: AppInfo;
  systemStatus: SystemStatus;
  connectionState: ConnectionState;
  /** Agents that arrived over Tauri events (rendered live in the viewport). */
  liveAgents: AgentDescriptor[];
  /** Backend-pushed activity, newest first (merged with mock feed in the UI). */
  liveFeed: ActivityEvent[];
  /** Most recent policy verdict (shown in the inspector + dashboard). */
  lastEvaluation: PolicyEvaluation | null;
  /** Bumped on every `evidenceUpdated` event (ledger badge / lists refresh). */
  evidenceVersion: number;
  /** True while a diagnostic round-trip is in flight. */
  diagnosticRunning: boolean;
  /** Durable queue tasks (live, from Tauri events + list_tasks refresh). */
  liveTasks: TaskQueueEntry[];
  /** Computer-use safety envelope (emergency stop latch, rate usage). */
  computerSafety: ComputerSafetyStatus | null;
  /** Latest TypeCompact receipt (ledger-persisted, proof-hashed). */
  lastContextReceipt: ContextCompactionReceipt | null;
  /** Durable-storage health (`unknown` outside Tauri or before first load). */
  storageHealth: "unknown" | "healthy" | "degraded";
  setAppInfo: (info: AppInfo) => void;
  setConnectionState: (state: ConnectionState) => void;
  setSystemStatus: (status: SystemStatus) => void;
  appendLiveAgent: (agent: AgentDescriptor) => void;
  appendFeed: (event: ActivityEvent) => void;
  setLastEvaluation: (evaluation: PolicyEvaluation | null) => void;
  bumpEvidenceVersion: () => void;
  setDiagnosticRunning: (running: boolean) => void;
  setLiveTasks: (tasks: TaskQueueEntry[]) => void;
  upsertLiveTask: (task: TaskQueueEntry) => void;
  setComputerSafety: (safety: ComputerSafetyStatus | null) => void;
  setLastContextReceipt: (receipt: ContextCompactionReceipt | null) => void;
  setStorageHealth: (health: "unknown" | "healthy" | "degraded") => void;
}

const INITIAL_STATUS: SystemStatus = {
  version: "0.2.0",
  uptimeSeconds: 0,
  activeAgents: 2,
  activeTasks: 2,
  policyBrokerHealthy: true,
  evidenceLedgerHealthy: true,
  lastAuditAt: "2026-10-31T12:00:00.000Z",
};

const MAX_FEED = 50;

export const useAppStore = create<AppState>()((set) => ({
  appInfo: { name: "Talos", version: "0.2.0", environment: "local" },
  systemStatus: INITIAL_STATUS,
  connectionState: "connecting",
  liveAgents: [],
  liveFeed: [],
  lastEvaluation: null,
  evidenceVersion: 0,
  diagnosticRunning: false,
  liveTasks: [],
  computerSafety: null,
  lastContextReceipt: null,
  storageHealth: "unknown",
  setAppInfo: (appInfo) => {
    set({ appInfo });
  },
  setConnectionState: (connectionState) => {
    set({ connectionState });
  },
  setSystemStatus: (systemStatus) => {
    set({ systemStatus });
  },
  appendLiveAgent: (agent) => {
    set((state) =>
      state.liveAgents.some((existing) => existing.id === agent.id)
        ? state
        : { liveAgents: [...state.liveAgents, agent] },
    );
  },
  appendFeed: (event) => {
    set((state) => ({
      liveFeed: [event, ...state.liveFeed].slice(0, MAX_FEED),
    }));
  },
  setLastEvaluation: (lastEvaluation) => {
    set({ lastEvaluation });
  },
  bumpEvidenceVersion: () => {
    set((state) => ({ evidenceVersion: state.evidenceVersion + 1 }));
  },
  setDiagnosticRunning: (diagnosticRunning) => {
    set({ diagnosticRunning });
  },
  setLiveTasks: (liveTasks) => {
    set({ liveTasks });
  },
  upsertLiveTask: (task) => {
    set((state) => {
      const existing = state.liveTasks.findIndex((t) => t.id === task.id);
      if (existing === -1) {
        return { liveTasks: [task, ...state.liveTasks] };
      }
      const liveTasks = [...state.liveTasks];
      liveTasks[existing] = task;
      return { liveTasks };
    });
  },
  setComputerSafety: (computerSafety) => {
    set({ computerSafety });
  },
  setLastContextReceipt: (lastContextReceipt) => {
    set({ lastContextReceipt });
  },
  setStorageHealth: (storageHealth) => {
    set({ storageHealth });
  },
}));
