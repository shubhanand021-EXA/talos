import type { SystemStatus } from "@talos/contracts";
import { create } from "zustand";
import type { ConnectionState } from "../types/index.js";

export interface AppInfo {
  name: string;
  version: string;
  environment: "local" | "staging" | "production";
}

interface AppState {
  appInfo: AppInfo;
  systemStatus: SystemStatus;
  connectionState: ConnectionState;
  setConnectionState: (state: ConnectionState) => void;
  setSystemStatus: (status: SystemStatus) => void;
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

export const useAppStore = create<AppState>()((set) => ({
  appInfo: { name: "Talos", version: "0.1.0", environment: "local" },
  systemStatus: INITIAL_STATUS,
  connectionState: "connected",
  setConnectionState: (connectionState) => {
    set({ connectionState });
  },
  setSystemStatus: (systemStatus) => {
    set({ systemStatus });
  },
}));
