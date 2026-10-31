import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ToolsPage } from "./ToolsPage.js";

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
    id: "fs.read",
    name: "fs.read",
    version: "0.1.0",
    description: "Read a file",
    capabilities: ["ReadFile"],
    riskLevel: "Low",
    reversible: true,
    requiresEvidence: false,
    requiresCompensation: false,
    enabledByDefault: true,
    inputSchema: { type: "object" },
    outputSchema: { type: "object" },
  },
  {
    id: "process.exec",
    name: "process.exec",
    version: "0.1.0",
    description: "Execute a subprocess",
    capabilities: ["ExecuteCommand"],
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

describe("ToolsPage", () => {
  it("renders the catalog with risk and enablement", async () => {
    render(<ToolsPage />);
    const names = await screen.findAllByText("fs.read");
    expect(names.length).toBeGreaterThan(0);
    expect(screen.getByText("process.exec")).toBeInTheDocument();
    expect(screen.getAllByText("Low").length).toBeGreaterThan(0);
    expect(screen.getByText("opt-in")).toBeInTheDocument();
  });

  it("shows a validation hint for invalid JSON params", async () => {
    const user = userEvent.setup();
    render(<ToolsPage />);
    const names = await screen.findAllByText("fs.read");
    expect(names.length).toBeGreaterThan(0);
    const params = screen.getByLabelText("Params (JSON)");
    fireEvent.change(params, { target: { value: "{not json" } });
    await user.click(screen.getByRole("button", { name: "Dry run" }));
    expect(
      await screen.findByText("Params must be valid JSON."),
    ).toBeInTheDocument();
  });

  it("shows an empty receipt before any invocation", async () => {
    render(<ToolsPage />);
    expect(await screen.findByText("No receipt yet")).toBeInTheDocument();
    await waitFor(() => {
      expect(mockListTools).toHaveBeenCalled();
    });
  });
});
