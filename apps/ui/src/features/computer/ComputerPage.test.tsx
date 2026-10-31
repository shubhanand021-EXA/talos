import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ComputerPage } from "./ComputerPage.js";

const { mockListTools, mockDescribeTool } = vi.hoisted(() => ({
  mockListTools: vi.fn(),
  mockDescribeTool: vi.fn(),
}));

vi.mock("../../bridge/index.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../bridge/index.js")>();
  return {
    ...actual,
    listTools: mockListTools,
    describeTool: mockDescribeTool,
    getComputerSafetyStatus: vi.fn().mockResolvedValue({
      enabled: false,
      dryRunDefault: true,
      emergencyStopped: false,
      actionsThisMinute: 0,
      maxActionsPerMinute: 30,
      viewportWidth: 1440,
      viewportHeight: 900,
      minConfidence: 0.7,
    }),
    computerEmergencyStop: vi.fn(),
    computerResetStop: vi.fn(),
  };
});

const CATALOG = [
  {
    id: "computer.capture_screen",
    name: "computer.capture_screen",
    version: "0.1.0",
    description: "Capture",
    capabilities: ["UiAction"],
    riskLevel: "Low",
    reversible: true,
    requiresEvidence: false,
    requiresCompensation: false,
    enabledByDefault: false,
    inputSchema: { type: "object" },
    outputSchema: { type: "object" },
  },
  {
    id: "computer.click",
    name: "computer.click",
    version: "0.1.0",
    description: "Click",
    capabilities: ["UiAction"],
    riskLevel: "High",
    reversible: false,
    requiresEvidence: true,
    requiresCompensation: false,
    enabledByDefault: false,
    inputSchema: { type: "object" },
    outputSchema: { type: "object" },
  },
];

beforeEach(() => {
  mockListTools.mockReset();
  mockDescribeTool.mockReset();
  mockListTools.mockResolvedValue(CATALOG);
  mockDescribeTool.mockImplementation((id: string) =>
    Promise.resolve(CATALOG.find((tool) => tool.id === id)),
  );
});

describe("ComputerPage", () => {
  it("renders the computer catalog with risk badges", async () => {
    render(<ComputerPage />);
    const names = await screen.findAllByText("computer.capture_screen");
    expect(names.length).toBeGreaterThan(0);
    expect(screen.getAllByText("computer.click").length).toBeGreaterThan(0);
  });

  it("shows a validation hint for invalid JSON params", async () => {
    const user = userEvent.setup();
    render(<ComputerPage />);
    const names = await screen.findAllByText("computer.capture_screen");
    expect(names.length).toBeGreaterThan(0);
    const params = screen.getByLabelText(
      "Params (JSON, sandboxed to the virtual screen)",
    );
    fireEvent.change(params, { target: { value: "{not json" } });
    await user.click(screen.getByRole("button", { name: "Dry run" }));
    expect(
      await screen.findByText("Params must be valid JSON."),
    ).toBeInTheDocument();
  });

  it("shows safety warnings for mutation actions", async () => {
    const user = userEvent.setup();
    render(<ComputerPage />);
    const names = await screen.findAllByText("computer.capture_screen");
    expect(names.length).toBeGreaterThan(0);
    await user.selectOptions(
      screen.getByLabelText("Action"),
      "computer.type_text",
    );
    expect(await screen.findByText(/Consequential action/)).toBeInTheDocument();
  });

  it("shows an emergency stop control with safety copy", async () => {
    render(<ComputerPage />);
    expect(
      await screen.findByRole("button", { name: "Emergency stop" }),
    ).toBeInTheDocument();
    expect(
      await screen.findByText(/never resumes canceled tasks/),
    ).toBeInTheDocument();
  });
});
