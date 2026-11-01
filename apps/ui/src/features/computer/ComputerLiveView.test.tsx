import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ComputerPage } from "./ComputerPage.js";

const { mockEmergencyStop, mockSetSessionScope, mockGetSessionScope } =
  vi.hoisted(() => ({
    mockEmergencyStop: vi.fn(),
    mockSetSessionScope: vi.fn(),
    mockGetSessionScope: vi.fn(),
  }));

vi.mock("../../bridge/index.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../bridge/index.js")>();
  return {
    ...actual,
    listTools: vi.fn().mockResolvedValue([
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
    ]),
    describeTool: vi.fn().mockResolvedValue(undefined),
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
    getComputerSessionScope: mockGetSessionScope,
    setComputerSessionScope: mockSetSessionScope,
    computerEmergencyStop: mockEmergencyStop,
    computerResetStop: vi.fn(),
    invokeTool: vi.fn().mockResolvedValue({
      id: "receipt-live-1",
      toolId: "computer.click",
      taskId: "task-computer-1",
      status: "success",
      dryRun: false,
      policyDecision: "allow",
      policyReason: "allowed",
      evidenceId: "ev-1",
      compensationPlanId: null,
      error: null,
      executedAt: "2026-11-01T00:00:00.000Z",
      output: {
        simulated: false,
        backend: "win32-uia",
        screenHash: "sha256:after",
        beforeHash: "sha256:before",
        verified: true,
        verificationFailed: false,
        focusLost: false,
        boundingBox: { x: 100, y: 100, width: 80, height: 40 },
        confidence: 0.95,
        element: { role: "button", name: "OK" },
        thumbnail: "data:image/png;base64,iVBOR",
        viewport: { width: 1440, height: 900 },
      },
    }),
    invokeToolDryRun: vi.fn().mockResolvedValue({
      id: "receipt-dry-1",
      toolId: "computer.click",
      taskId: "task-computer-1",
      status: "success",
      dryRun: true,
      policyDecision: "allow",
      policyReason: "allowed",
      evidenceId: "ev-1",
      compensationPlanId: null,
      error: null,
      executedAt: "2026-11-01T00:00:00.000Z",
      output: {},
    }),
  };
});

beforeEach(() => {
  mockEmergencyStop.mockReset();
  mockSetSessionScope.mockReset();
  mockGetSessionScope.mockReset();
  mockGetSessionScope.mockResolvedValue({
    allowedApps: ["notes"],
    blockedApps: [],
  });
  mockEmergencyStop.mockResolvedValue({
    enabled: false,
    dryRunDefault: true,
    emergencyStopped: true,
    actionsThisMinute: 0,
    maxActionsPerMinute: 30,
    viewportWidth: 1440,
    viewportHeight: 900,
    minConfidence: 0.7,
  });
  mockSetSessionScope.mockImplementation((scope: unknown) =>
    Promise.resolve(scope),
  );
});

describe("Computer live view", () => {
  it("renders the live screen panel with thumbnail and bounding box", async () => {
    const user = userEvent.setup();
    render(<ComputerPage />);
    await screen.findByText("computer.capture_screen");
    await user.selectOptions(screen.getByLabelText("Action"), "computer.click");
    await user.click(screen.getByRole("button", { name: "Invoke live" }));
    const thumbnail = await screen.findByAltText("Last captured screen");
    expect(thumbnail.getAttribute("src")).toMatch(/^data:image\/png;base64,/);
    expect(
      screen.getByLabelText("Resolved element bounding box"),
    ).toBeInTheDocument();
    expect(screen.getByText(/confidence 0\.95/)).toBeInTheDocument();
  });

  it("appends executed actions to the timeline with hashes", async () => {
    const user = userEvent.setup();
    render(<ComputerPage />);
    await screen.findByText("computer.capture_screen");
    await user.selectOptions(screen.getByLabelText("Action"), "computer.click");
    await user.click(screen.getByRole("button", { name: "Invoke live" }));
    const timeline = await screen.findByRole("list", {
      name: "Computer action timeline",
    });
    expect(within(timeline).getByText("verified")).toBeInTheDocument();
  });

  it("take control stops Talos input and explains the handoff", async () => {
    const user = userEvent.setup();
    render(<ComputerPage />);
    await screen.findByRole("button", { name: "Take Control" });
    await user.click(screen.getByRole("button", { name: "Take Control" }));
    expect(mockEmergencyStop).toHaveBeenCalledTimes(1);
    expect(await screen.findByText(/Control is yours/)).toBeInTheDocument();
  });

  it("saves the application allowlist through the session scope", async () => {
    const user = userEvent.setup();
    render(<ComputerPage />);
    const allowed = await screen.findByLabelText(/Allowed applications/);
    expect(allowed).toHaveValue("notes");
    fireEvent.change(allowed, { target: { value: "notes, calculator" } });
    await user.click(screen.getByRole("button", { name: "Save scope" }));
    expect(mockSetSessionScope).toHaveBeenCalledWith({
      allowedApps: ["notes", "calculator"],
      blockedApps: [],
    });
    expect(await screen.findByText(/Session scope saved/)).toBeInTheDocument();
  });
});
