import { beforeEach, describe, expect, it, vi } from "vitest";

const { mockInvoke, mockListen } = vi.hoisted(() => ({
  mockInvoke: vi.fn(),
  mockListen: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: mockInvoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mockListen }));

import {
  AgentSpawnedPayloadSchema,
  BridgeError,
  DiagnosticSummarySchema,
  getAppInfo,
  getSystemStatus,
  isTauri,
  runDiagnostic,
  subscribeTalosEvent,
  verifyEvidenceLedger,
} from "./index.js";

const EVALUATION = {
  id: "eval-diag-req-1",
  actionRequestId: "diag-req-1",
  decision: "allow",
  reason: "allowed: low-risk read",
  policyVersion: "talos-policy-broker/1",
  requiredChecks: [],
  evaluatedAt: "2026-10-31T12:00:00.000Z",
};

beforeEach(() => {
  vi.unstubAllGlobals();
  mockInvoke.mockReset();
  mockListen.mockReset();
});

describe("bridge client", () => {
  it("detects the Tauri container", () => {
    expect(isTauri()).toBe(false);
    vi.stubGlobal("__TAURI_INTERNALS__", {});
    expect(isTauri()).toBe(true);
  });

  it("raises backend_unavailable outside Tauri", async () => {
    await expect(getSystemStatus()).rejects.toMatchObject({
      code: "backend_unavailable",
    });
    expect(mockInvoke).not.toHaveBeenCalled();
  });

  it("validates get_app_info responses", async () => {
    vi.stubGlobal("__TAURI_INTERNALS__", {});
    mockInvoke.mockResolvedValueOnce({
      name: "Talos",
      version: "0.1.0",
      environment: "local",
    });
    await expect(getAppInfo()).resolves.toMatchObject({ name: "Talos" });
    mockInvoke.mockResolvedValueOnce({ name: "Talos" });
    await expect(getAppInfo()).rejects.toMatchObject({
      code: "validation_failed",
    });
  });

  it("validates system status and ledger shapes", async () => {
    vi.stubGlobal("__TAURI_INTERNALS__", {});
    mockInvoke.mockResolvedValueOnce({
      version: "0.1.0",
      uptimeSeconds: 12,
      activeAgents: 1,
      activeTasks: 1,
      policyBrokerHealthy: true,
      evidenceLedgerHealthy: true,
      lastAuditAt: null,
    });
    await expect(getSystemStatus()).resolves.toMatchObject({
      activeAgents: 1,
    });
    mockInvoke.mockResolvedValueOnce({ healthy: true });
    await expect(verifyEvidenceLedger()).rejects.toMatchObject({
      code: "validation_failed",
    });
  });

  it("validates diagnostic summaries end to end", async () => {
    vi.stubGlobal("__TAURI_INTERNALS__", {});
    mockInvoke.mockResolvedValueOnce({
      total: 20,
      passed: 20,
      checks: [{ name: "task_created", ok: true, detail: "ok" }],
      ledger: {
        healthy: true,
        event_count: 1,
        evidence_count: 3,
        claim_count: 1,
        detail: "ok",
        checked_at: "2026-10-31T12:00:00.000Z",
        chain_valid: true,
        chain_checked: 1,
        chain_last_seq: 1,
        chain_last_hash: "sha256:abc",
        chain_first_bad_seq: null,
        recent_events: [],
        recent_evidence: [],
        recent_claims: [],
        compensation_plans: [],
      },
      task_id: "diag-task-1",
      agent_id: "diag-agent-1",
      evaluation: EVALUATION,
      claim_id: "diag-claim-1",
      plan_id: "diag-plan-1",
    });
    const summary = await runDiagnostic();
    expect(DiagnosticSummarySchema.safeParse(summary).success).toBe(true);
    expect(summary.evaluation.decision).toBe("allow");
  });

  it("maps invoke rejections to invoke_rejected", async () => {
    vi.stubGlobal("__TAURI_INTERNALS__", {});
    mockInvoke.mockRejectedValueOnce(new Error("boom"));
    await expect(getAppInfo()).rejects.toMatchObject({
      code: "invoke_rejected",
    });
    expect(BridgeError).toBeDefined();
  });
});

describe("event subscription", () => {
  it("refuses to subscribe outside Tauri", async () => {
    await expect(
      subscribeTalosEvent("heartbeat", () => {}),
    ).rejects.toMatchObject({ code: "backend_unavailable" });
    expect(mockListen).not.toHaveBeenCalled();
  });

  it("forwards validated payloads and drops invalid ones", async () => {
    vi.stubGlobal("__TAURI_INTERNALS__", {});
    let captured: ((event: { payload: unknown }) => void) | undefined;
    mockListen.mockImplementationOnce(
      (event: string, handler: (e: { payload: unknown }) => void) => {
        expect(event).toBe("agentSpawned");
        captured = handler;
        return Promise.resolve(() => {});
      },
    );
    const seen: unknown[] = [];
    const unlisten = await subscribeTalosEvent(
      "agentSpawned",
      (payload) => {
        seen.push(payload);
      },
      AgentSpawnedPayloadSchema,
    );
    expect(typeof unlisten).toBe("function");
    captured?.({
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
    captured?.({ payload: { agent: { id: 42 } } });
    expect(seen).toHaveLength(1);
  });

  it("maps listen failures to subscription_failed", async () => {
    vi.stubGlobal("__TAURI_INTERNALS__", {});
    mockListen.mockRejectedValueOnce(new Error("nope"));
    await expect(
      subscribeTalosEvent("heartbeat", () => {}),
    ).rejects.toMatchObject({ code: "subscription_failed" });
  });
});
