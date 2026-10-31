import { render, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { mockInvoke, mockListen } = vi.hoisted(() => ({
  mockInvoke: vi.fn(),
  mockListen: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: mockInvoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mockListen }));

import { useTalosSync } from "./useTalosSync.js";
import { useAppStore } from "../stores/appStore.js";

const STATUS = {
  version: "0.1.0",
  uptimeSeconds: 42,
  activeAgents: 1,
  activeTasks: 1,
  policyBrokerHealthy: true,
  evidenceLedgerHealthy: true,
  lastAuditAt: null,
};

function Probe() {
  useTalosSync();
  return null;
}

beforeEach(() => {
  vi.unstubAllGlobals();
  mockInvoke.mockReset();
  mockListen.mockReset();
  useAppStore.setState({
    connectionState: "connected",
    liveAgents: [],
    liveFeed: [],
    lastEvaluation: null,
    evidenceVersion: 0,
  });
});

describe("useTalosSync", () => {
  it("marks the link disconnected outside Tauri", async () => {
    render(<Probe />);
    await waitFor(() => {
      expect(useAppStore.getState().connectionState).toBe("disconnected");
    });
    expect(mockInvoke).not.toHaveBeenCalled();
  });

  it("hydrates stores and propagates simulated events", async () => {
    vi.stubGlobal("__TAURI_INTERNALS__", {});
    mockInvoke.mockImplementation((command: string) => {
      if (command === "get_app_info") {
        return Promise.resolve({
          name: "Talos",
          version: "0.1.0",
          environment: "local",
        });
      }
      return Promise.resolve(STATUS);
    });
    const handlers = new Map<string, (event: { payload: unknown }) => void>();
    mockListen.mockImplementation(
      (event: string, handler: (e: { payload: unknown }) => void) => {
        handlers.set(event, handler);
        return Promise.resolve(() => {});
      },
    );

    render(<Probe />);
    await waitFor(() => {
      expect(useAppStore.getState().systemStatus.uptimeSeconds).toBe(42);
    });

    handlers.get("agentSpawned")?.({
      payload: {
        agent: {
          id: "diag-agent-1",
          displayName: "Echo Unit 1",
          role: "researcher",
          status: "running",
          sessionId: "session-local",
          capabilities: ["fs.read"],
          createdAt: "2026-10-31T12:00:00.000Z",
          updatedAt: "2026-10-31T12:00:00.000Z",
        },
      },
    });
    expect(useAppStore.getState().liveAgents).toHaveLength(1);

    handlers.get("policyEvaluated")?.({
      payload: {
        evaluation: {
          id: "eval-diag-req-1",
          actionRequestId: "diag-req-1",
          decision: "allow",
          reason: "allowed",
          policyVersion: "talos-policy-broker/1",
          requiredChecks: [],
          evaluatedAt: "2026-10-31T12:00:00.000Z",
        },
      },
    });
    expect(useAppStore.getState().lastEvaluation?.decision).toBe("allow");

    handlers.get("evidenceUpdated")?.({
      payload: {
        evidence: {
          id: "diag-ev-1",
          kind: "log",
          status: "pending",
          payload: { decision: "allow" },
          createdAt: "2026-10-31T12:00:00.000Z",
        },
      },
    });
    expect(useAppStore.getState().evidenceVersion).toBe(1);
    expect(useAppStore.getState().liveFeed.length).toBeGreaterThan(0);

    handlers.get("heartbeat")?.({
      payload: {
        source: "talos-desktop",
        timestamp: "2026-10-31T12:00:05.000Z",
        sequence: 1,
        uptimeSeconds: 99,
      },
    });
    expect(useAppStore.getState().systemStatus.uptimeSeconds).toBe(99);
  });
});
