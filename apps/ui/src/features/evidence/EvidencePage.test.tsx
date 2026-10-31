import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { mockInvoke } = vi.hoisted(() => ({
  mockInvoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: mockInvoke }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

import { EvidencePage } from "./EvidencePage.js";
import { useAppStore } from "../../stores/appStore.js";

const LEDGER = {
  healthy: true,
  event_count: 2,
  evidence_count: 1,
  claim_count: 1,
  detail: "chains verified (2 event links, 1 evidence rows)",
  checked_at: "2026-10-31T12:00:00.000Z",
  chain_valid: true,
  chain_checked: 2,
  chain_last_seq: 2,
  chain_last_hash: "sha256:abc",
  chain_first_bad_seq: null,
  recent_events: [
    {
      id: "diag-evt-1",
      kind: "diagnostic",
      seq: 2,
      created_at: "2026-10-31T12:00:00.000Z",
    },
  ],
  recent_evidence: [
    {
      id: "diag-ev-1",
      kind: "log",
      status: "verified",
      hash: "sha256:abc",
      created_at: "2026-10-31T12:00:00.000Z",
    },
  ],
  recent_claims: [
    {
      id: "diag-claim-1",
      statement: "diagnostic run 1 completed per policy",
      status: "verified",
      created_at: "2026-10-31T12:00:00.000Z",
    },
  ],
  compensation_plans: [
    {
      id: "diag-plan-1",
      action_request_id: "diag-rotate-1",
      strategy: "rollback_command",
      status: "proposed",
      updated_at: "2026-10-31T12:00:00.000Z",
    },
  ],
};

function renderPage() {
  return render(
    <MemoryRouter>
      <EvidencePage />
    </MemoryRouter>,
  );
}

beforeEach(() => {
  vi.unstubAllGlobals();
  mockInvoke.mockReset();
  useAppStore.setState({ liveFeed: [], evidenceVersion: 0 });
});

describe("EvidencePage", () => {
  it("renders the mock snapshot outside Tauri", () => {
    renderPage();
    expect(
      screen.getByText("mock snapshot", { exact: false }),
    ).toBeInTheDocument();
    expect(screen.getByText("ev-1041")).toBeInTheDocument();
    expect(mockInvoke).not.toHaveBeenCalled();
  });

  it("renders live integrity state inside Tauri", async () => {
    vi.stubGlobal("__TAURI_INTERNALS__", {});
    mockInvoke.mockResolvedValue(LEDGER);
    renderPage();
    expect(await screen.findByText("chain valid")).toBeInTheDocument();
    expect(screen.getByText("2 links checked")).toBeInTheDocument();
    expect(screen.getByTitle("diag-claim-1")).toBeInTheDocument();
    expect(screen.getByText("rollback_command")).toBeInTheDocument();
  });

  it("surfaces a broken chain without hiding it", async () => {
    vi.stubGlobal("__TAURI_INTERNALS__", {});
    mockInvoke.mockResolvedValue({
      ...LEDGER,
      healthy: false,
      chain_valid: false,
      chain_first_bad_seq: 2,
      detail: "chain broken at seq 2",
    });
    renderPage();
    expect(await screen.findByText("CHAIN BROKEN")).toBeInTheDocument();
    expect(screen.getByText("first bad seq 2")).toBeInTheDocument();
  });

  it("shows loading then error states with retry", async () => {
    vi.stubGlobal("__TAURI_INTERNALS__", {});
    mockInvoke.mockRejectedValueOnce(new Error("ledger offline"));
    const user = userEvent.setup();
    renderPage();
    expect(await screen.findByText("Ledger unreachable")).toBeInTheDocument();
    mockInvoke.mockResolvedValueOnce(LEDGER);
    await user.click(screen.getByRole("button", { name: "Retry" }));
    expect(await screen.findByText("chain valid")).toBeInTheDocument();
  });

  it("verify action records a feed entry and bumps the version", async () => {
    vi.stubGlobal("__TAURI_INTERNALS__", {});
    mockInvoke.mockResolvedValue(LEDGER);
    const user = userEvent.setup();
    renderPage();
    await user.click(
      await screen.findByRole("button", { name: "Verify Ledger" }),
    );
    await waitFor(() => {
      expect(useAppStore.getState().evidenceVersion).toBeGreaterThan(0);
    });
    expect(
      useAppStore
        .getState()
        .liveFeed.some((event) => event.actor === "evidence-store"),
    ).toBe(true);
  });
});
