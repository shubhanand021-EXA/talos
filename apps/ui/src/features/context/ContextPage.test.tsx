import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ContextPage } from "./ContextPage.js";
import { BridgeError } from "../../bridge/errors.js";

const { mockBudget, mockCompact, mockDiagnostic } = vi.hoisted(() => ({
  mockBudget: vi.fn(),
  mockCompact: vi.fn(),
  mockDiagnostic: vi.fn(),
}));

vi.mock("../../bridge/index.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../bridge/index.js")>();
  return {
    ...actual,
    getContextBudgetStatus: mockBudget,
    compactTaskContext: mockCompact,
    compactDiagnosticContext: mockDiagnostic,
  };
});

const STATUS = {
  taskId: "task-1",
  totalUsed: 120,
  totalBudget: 8000,
  overflow: false,
  layers: [
    {
      layer: "safety",
      usedTokens: 10,
      maxTokens: 800,
      overflow: false,
      protected: true,
      segmentCount: 1,
    },
    {
      layer: "scratchpad",
      usedTokens: 5,
      maxTokens: 800,
      overflow: false,
      protected: false,
      segmentCount: 1,
    },
  ],
};

const RECEIPT = {
  id: "compact-task-1-abc",
  taskId: "task-1",
  originalTokens: 200,
  compactedTokens: 120,
  preservedInvariants: ["ids:3/3", "safety:unchanged"],
  droppedSegments: [],
  trimmedSegments: ["scratch-task-1"],
  proofHash: "sha256:abc",
  segments: [
    {
      id: "sys-task-1",
      layer: "system",
      body: "system prompt",
      originalTokens: 4,
      compactedTokens: 4,
      trimmed: false,
      dropped: false,
      protected: true,
    },
  ],
  version: "typecompact/1",
  executedAt: "2026-10-31T00:00:00.000Z",
};

beforeEach(() => {
  mockBudget.mockReset();
  mockCompact.mockReset();
  mockDiagnostic.mockReset();
  mockBudget.mockResolvedValue(STATUS);
  mockCompact.mockResolvedValue(RECEIPT);
  mockDiagnostic.mockResolvedValue(RECEIPT);
});

describe("ContextPage", () => {
  it("renders budget bars for every returned layer", async () => {
    render(<ContextPage />);
    expect(await screen.findByText("safety")).toBeInTheDocument();
    expect(screen.getByText("scratchpad")).toBeInTheDocument();
    expect(
      screen.getByRole("progressbar", { name: "safety budget" }),
    ).toBeInTheDocument();
  });

  it("shows a backend hint when the budget load fails", async () => {
    mockBudget.mockRejectedValueOnce(
      new BridgeError("backend_unavailable", "offline"),
    );
    render(<ContextPage />);
    expect(
      await screen.findByText(
        "Backend unavailable — launch the desktop shell.",
      ),
    ).toBeInTheDocument();
  });

  it("compacts and shows the invariant checklist plus receipt", async () => {
    const user = userEvent.setup();
    render(<ContextPage />);
    expect(await screen.findByText("safety")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Compact" }));
    expect(await screen.findByText("ids:3/3")).toBeInTheDocument();
    expect(screen.getByText("sha256:abc")).toBeInTheDocument();
  });

  it("accepts a custom task id and budget", async () => {
    const user = userEvent.setup();
    render(<ContextPage />);
    expect(await screen.findByText("safety")).toBeInTheDocument();
    await user.clear(screen.getByLabelText("Task id (ledger linkage)"));
    await user.type(
      screen.getByLabelText("Task id (ledger linkage)"),
      "task-9",
    );
    fireEvent.change(
      screen.getByLabelText("Total budget (tokens, default 8000)"),
      {
        target: { value: "100" },
      },
    );
    await user.click(screen.getByRole("button", { name: "Compact" }));
    expect(mockCompact).toHaveBeenCalledWith("task-9", 100);
  });
});
