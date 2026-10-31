import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { BrowserPage } from "./BrowserPage.js";

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
  mockListTools.mockResolvedValue(CATALOG);
  mockDescribeTool.mockImplementation((id: string) =>
    Promise.resolve(CATALOG.find((tool) => tool.id === id)),
  );
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
});
