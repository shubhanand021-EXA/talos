import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ToolRagPanel } from "./ToolRagPanel.js";

const { mockQuery } = vi.hoisted(() => ({
  mockQuery: vi.fn(),
}));

vi.mock("../../bridge/index.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../bridge/index.js")>();
  return {
    ...actual,
    queryToolCandidates: mockQuery,
  };
});

const CANDIDATES = [
  {
    toolId: "fs.read",
    score: 4.5,
    reason: "keyword overlap 2 token(s); risk Low within ceiling any",
    matchedFields: ["name", "description"],
    riskLevel: "Low",
    requiredApprovals: [],
  },
  {
    toolId: "process.exec",
    score: 1.0,
    reason: "no lexical match; baseline candidate",
    matchedFields: [],
    riskLevel: "High",
    requiredApprovals: ["human_review"],
  },
];

beforeEach(() => {
  mockQuery.mockReset();
  mockQuery.mockResolvedValue(CANDIDATES);
});

describe("ToolRagPanel", () => {
  it("renders candidates with score, reason, and risk badge", async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();
    render(<ToolRagPanel onSelectTool={onSelect} />);
    await user.click(screen.getByRole("button", { name: "Rank tools" }));
    expect(await screen.findByText("fs.read")).toBeInTheDocument();
    expect(screen.getByText("4.50")).toBeInTheDocument();
    expect(
      screen.getByText(/keyword overlap 2 token\(s\)/),
    ).toBeInTheDocument();
    expect(screen.getByText("Low")).toBeInTheDocument();
    expect(screen.getByText("matched: name, description")).toBeInTheDocument();
  });

  it("links a candidate to the tool manifest", async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();
    render(<ToolRagPanel onSelectTool={onSelect} />);
    await user.click(screen.getByRole("button", { name: "Rank tools" }));
    await user.click(
      await screen.findByRole("button", { name: "Select fs.read" }),
    );
    expect(onSelect).toHaveBeenCalledWith("fs.read");
  });

  it("shows an empty state when filters exclude everything", async () => {
    const user = userEvent.setup();
    mockQuery.mockResolvedValueOnce([]);
    render(<ToolRagPanel onSelectTool={() => {}} />);
    await user.click(screen.getByRole("button", { name: "Rank tools" }));
    expect(await screen.findByText("No candidates")).toBeInTheDocument();
  });

  it("shows a backend hint when the query fails", async () => {
    const user = userEvent.setup();
    const { BridgeError } = await import("../../bridge/errors.js");
    mockQuery.mockRejectedValueOnce(
      new BridgeError("backend_unavailable", "offline"),
    );
    render(<ToolRagPanel onSelectTool={() => {}} />);
    await user.click(screen.getByRole("button", { name: "Rank tools" }));
    expect(
      await screen.findByText(
        "Backend unavailable — launch the desktop shell.",
      ),
    ).toBeInTheDocument();
  });
});
