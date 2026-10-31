import type {
  AgentDescriptor,
  PolicyEvaluation,
  SystemStatus,
} from "@talos/contracts";
import { create } from "zustand";
import type { ActivityEvent, ConnectionState } from "../types/index.js";

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
  setAppInfo: (info: AppInfo) => void;
  setConnectionState: (state: ConnectionState) => void;
  setSystemStatus: (status: SystemStatus) => void;
  appendLiveAgent: (agent: AgentDescriptor) => void;
  appendFeed: (event: ActivityEvent) => void;
  setLastEvaluation: (evaluation: PolicyEvaluation | null) => void;
  bumpEvidenceVersion: () => void;
  setDiagnosticRunning: (running: boolean) => void;
}

const INITIAL_STATUS: SystemStatus = {
  version: "0.1.0",
  uptimeSeconds: 0,
  activeAgents: 2,
  activeTasks: 2,
  policyBrokerHealthy: true,
  evidenceLedgerHealthy: true,
  lastAuditAt: "2026-10-31T12:00:00.000Z",
};

const MAX_FEED = 50;

export const useAppStore = create<AppState>()((set) => ({
  appInfo: { name: "Talos", version: "0.1.0", environment: "local" },
  systemStatus: INITIAL_STATUS,
  connectionState: "connected",
  liveAgents: [],
  liveFeed: [],
  lastEvaluation: null,
  evidenceVersion: 0,
  diagnosticRunning: false,
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
}));
