import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { BrowserPage } from "./BrowserPage.js";

const { mockListTools, mockDescribeTool, mockInvoke, mockSubscribe } =
  vi.hoisted(() => ({
    mockListTools: vi.fn(),
    mockDescribeTool: vi.fn(),
    mockInvoke: vi.fn(),
    mockSubscribe: vi.fn(),
  }));

vi.mock("../../bridge/index.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../bridge/index.js")>();
  return {
    ...actual,
    listTools: mockListTools,
    describeTool: mockDescribeTool,
    invokeTool: mockInvoke,
    invokeToolDryRun: mockInvoke,
    subscribeTalosEvent: mockSubscribe,
  };
});

const CATALOG = [
  {
    id: "browser.snapshot",
    name: "browser.snapshot",
    version: "0.1.0",
    description: "Snapshot",
    capabilities: ["OpenBrowser"],
    riskLevel: "Low",
    reversible: true,
    requiresEvidence: false,
    requiresCompensation: false,
    enabledByDefault: true,
    inputSchema: { type: "object" },
    outputSchema: { type: "object" },
  },
  {
    id: "browser.click",
    name: "browser.click",
    version: "0.1.0",
    description: "Click",
    capabilities: ["OpenBrowser"],
    riskLevel: "Medium",
    reversible: false,
    requiresEvidence: true,
    requiresCompensation: false,
    enabledByDefault: true,
    inputSchema: { type: "object" },
    outputSchema: { type: "object" },
  },
];

beforeEach(() => {
  mockListTools.mockReset();
  mockDescribeTool.mockReset();
  mockInvoke.mockReset();
  mockSubscribe.mockReset();
  mockListTools.mockResolvedValue(CATALOG);
  mockDescribeTool.mockImplementation((id: string) =>
    Promise.resolve(CATALOG.find((tool) => tool.id === id)),
  );
  mockInvoke.mockImplementation((toolId: string) =>
    Promise.resolve({
      id: "receipt-1",
      toolId,
      taskId: "task-browser-1",
      dryRun: false,
      status: "success",
      output: { simulated: true },
      policyDecision: "Allow",
      policyReason: "allowed",
      executedAt: "2026-11-01T00:00:00.000Z",
    }),
  );
  mockSubscribe.mockResolvedValue(() => undefined);
});

describe("BrowserPage", () => {
  it("renders the browser catalog with risk badges", async () => {
    render(<BrowserPage />);
    const names = await screen.findAllByText("browser.snapshot");
    expect(names.length).toBeGreaterThan(0);
    expect(screen.getAllByText("browser.click").length).toBeGreaterThan(0);
  });

  it("shows a validation hint for invalid JSON params", async () => {
    const user = userEvent.setup();
    render(<BrowserPage />);
    const names = await screen.findAllByText("browser.snapshot");
    expect(names.length).toBeGreaterThan(0);
    const params = screen.getByLabelText("Params (JSON)");
    fireEvent.change(params, { target: { value: "{not json" } });
    await user.click(screen.getByRole("button", { name: "Dry run" }));
    expect(
      await screen.findByText("Params must be valid JSON."),
    ).toBeInTheDocument();
  });

  it("shows safety warnings for mutation actions", async () => {
    const user = userEvent.setup();
    render(<BrowserPage />);
    const names = await screen.findAllByText("browser.snapshot");
    expect(names.length).toBeGreaterThan(0);
    await user.selectOptions(screen.getByLabelText("Action"), "browser.click");
    expect(await screen.findByText(/Consequential action/)).toBeInTheDocument();
  });

  it("toggles live mode off by default", async () => {
    render(<BrowserPage />);
    expect(await screen.findByText("browser.snapshot")).toBeTruthy();
    expect(screen.getByText("live: off (mock)")).toBeInTheDocument();
    const toggle = screen.getByRole("checkbox", { name: /Live mode/ });
    fireEvent.click(toggle);
    expect(await screen.findByText("live: opt-in")).toBeInTheDocument();
  });

  it("shows thumbnails and sensitive banners from live receipts", async () => {
    const user = userEvent.setup();
    render(<BrowserPage />);
    expect(await screen.findByText("browser.snapshot")).toBeInTheDocument();
    mockInvoke.mockResolvedValueOnce({
      id: "receipt-live",
      toolId: "browser.screenshot",
      taskId: "task-browser-1",
      dryRun: false,
      status: "success",
      output: {
        simulated: false,
        screenshotHash: "sha256:abc",
        domHash: "sha256:def",
        thumbnailDataUrl: "data:image/jpeg;base64,AAA",
        sensitive: true,
        redacted: true,
      },
      policyDecision: "Allow",
      policyReason: "allowed",
      executedAt: "2026-11-01T00:00:00.000Z",
    });
    await user.click(screen.getByRole("button", { name: "Take screenshot" }));
    expect(
      await screen.findByAltText("Live page thumbnail"),
    ).toBeInTheDocument();
    expect(await screen.findByText(/Sensitive page/)).toBeInTheDocument();
    expect(screen.getByText(/Password fields redacted/)).toBeInTheDocument();
  });

  it("closes sessions and surfaces worker crashes", async () => {
    const user = userEvent.setup();
    let crashHandler:
      ((payload: { code: string; taskId: string }) => void) | undefined;
    mockSubscribe.mockImplementation(
      (
        _event: string,
        handler: (payload: { code: string; taskId: string }) => void,
      ) => {
        crashHandler = handler;
        return Promise.resolve(() => undefined);
      },
    );
    render(<BrowserPage />);
    expect(await screen.findByText("browser.snapshot")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Close session" }));
    expect(mockInvoke).toHaveBeenCalledWith(
      "browser.session",
      expect.anything(),
      expect.objectContaining({ operation: "close" }),
    );
    crashHandler?.({
      code: "browser_worker_crashed",
      taskId: "task-browser-1",
    });
    expect(
      await screen.findByText(/Browser worker crashed/),
    ).toBeInTheDocument();
  });
});
