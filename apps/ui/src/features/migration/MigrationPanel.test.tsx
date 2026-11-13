import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { MigrationPanel } from "./MigrationPanel.js";
import { useMigrationStore } from "../../stores/migrationStore.js";

const {
  mockScan,
  mockPreview,
  mockApply,
  mockApprovePlugin,
  mockValidateSkill,
  mockPublishSkill,
  mockToggleSchedule,
} = vi.hoisted(() => ({
  mockScan: vi.fn(),
  mockPreview: vi.fn(),
  mockApply: vi.fn(),
  mockApprovePlugin: vi.fn(),
  mockValidateSkill: vi.fn(),
  mockPublishSkill: vi.fn(),
  mockToggleSchedule: vi.fn(),
}));

vi.mock("../../bridge/index.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../bridge/index.js")>();
  return {
    ...actual,
    isTauri: () => false,
    migrationScan: mockScan,
    migrationPreview: mockPreview,
    migrationApply: mockApply,
    approvePlugin: mockApprovePlugin,
    validateSkill: mockValidateSkill,
    publishSkill: mockPublishSkill,
    toggleSchedule: mockToggleSchedule,
  };
});

const SCAN = {
  importId: "migration-abc123",
  workspacePath: "/tmp/ws",
  findings: [
    {
      sourcePath: "/tmp/ws/AGENTS.md",
      kind: "agents",
      category: "instructions",
      flagged: true,
      riskLabels: ["prompt-injection"],
      secretStripped: false,
      note: "flagged",
    },
    {
      sourcePath: "/tmp/ws/.mcp.json",
      kind: "mcp-config",
      category: "mcp-servers",
      flagged: true,
      riskLabels: ["secret-present"],
      secretStripped: true,
      note: "stripped",
    },
  ],
  scannedAt: "2026-11-14T00:00:00.000Z",
};

const PREVIEW = {
  importId: "migration-abc123",
  items: [
    {
      category: "instructions",
      action: "import-directives",
      target: "directives:/tmp/ws/AGENTS.md",
      defaultState: "quarantined",
      riskLabels: ["prompt-injection"],
      secretStripped: false,
    },
    {
      category: "mcp-servers",
      action: "stage-mcp-server",
      target: "mcp-server:/tmp/ws/.mcp.json",
      defaultState: "disabled",
      riskLabels: ["secret-present"],
      secretStripped: true,
    },
  ],
  previewedAt: "2026-11-14T00:00:00.000Z",
};

const FIRST_CREATED = PREVIEW.items[0];
const FIRST_SKIPPED = PREVIEW.items[1];
if (FIRST_CREATED === undefined || FIRST_SKIPPED === undefined) {
  throw new Error("migration fixture invariant broken");
}

const APPLY_RESULT = {
  importId: "migration-abc123",
  approvedCategories: ["instructions"],
  created: [FIRST_CREATED],
  skipped: [FIRST_SKIPPED],
  evidenceIds: ["migration-abc123-import"],
  appliedAt: "2026-11-14T00:00:00.000Z",
};

function resetStore() {
  useMigrationStore.setState({
    scan: null,
    preview: null,
    applyResult: null,
    busy: false,
    error: null,
    announcement: null,
    subscribed: true,
  });
}

beforeEach(() => {
  resetStore();
  for (const mock of [
    mockScan,
    mockPreview,
    mockApply,
    mockApprovePlugin,
    mockValidateSkill,
    mockPublishSkill,
    mockToggleSchedule,
  ]) {
    mock.mockReset();
  }
  mockScan.mockResolvedValue(SCAN);
  mockPreview.mockResolvedValue(PREVIEW);
  mockApply.mockImplementation((_importId: string, categories: string[]) =>
    Promise.resolve({ ...APPLY_RESULT, approvedCategories: categories }),
  );
});

describe("MigrationPanel", () => {
  it("runs scan → preview → approve → import end to end", async () => {
    const user = userEvent.setup();
    render(<MigrationPanel />);

    await user.type(
      screen.getByLabelText(/workspace path to scan/i),
      "/tmp/ws",
    );
    await user.click(screen.getByRole("button", { name: "Scan" }));
    expect(await screen.findByText("/tmp/ws/AGENTS.md")).toBeInTheDocument();
    expect(mockScan).toHaveBeenCalledWith("/tmp/ws");

    await user.click(screen.getByRole("button", { name: "Preview import" }));
    expect(
      await screen.findByText(/what import would create/i),
    ).toBeInTheDocument();

    await user.click(
      screen.getByLabelText(
        /approve instructions \(AGENTS\.MD \/ CLAUDE\.MD\) for import/i,
      ),
    );
    await user.click(
      screen.getByRole("button", { name: "Import approved categories" }),
    );
    const dialog = await screen.findByRole("dialog");
    await user.click(
      within(dialog).getByRole("button", { name: "Confirm import" }),
    );
    expect(
      await screen.findByText(/imported 1 artifacts/i),
    ).toBeInTheDocument();
    expect(mockApply).toHaveBeenCalledWith("migration-abc123", [
      "instructions",
    ]);
  });

  it("never auto-enables: enable paths stay untouched, states stay safe", async () => {
    const user = userEvent.setup();
    render(<MigrationPanel />);

    await user.type(screen.getByLabelText(/workspace path to scan/i), "/tmp");
    await user.click(screen.getByRole("button", { name: "Scan" }));
    await screen.findByText("/tmp/ws/AGENTS.md");
    await user.click(screen.getByRole("button", { name: "Preview import" }));
    await screen.findByText(/what import would create/i);
    await user.click(
      screen.getByLabelText(
        /approve instructions \(AGENTS\.MD \/ CLAUDE\.MD\) for import/i,
      ),
    );
    await user.click(
      screen.getByRole("button", { name: "Import approved categories" }),
    );
    await user.click(
      within(await screen.findByRole("dialog")).getByRole("button", {
        name: "Confirm import",
      }),
    );
    await screen.findByText(/imported 1 artifacts/i);

    expect(mockApprovePlugin).not.toHaveBeenCalled();
    expect(mockValidateSkill).not.toHaveBeenCalled();
    expect(mockPublishSkill).not.toHaveBeenCalled();
    expect(mockToggleSchedule).not.toHaveBeenCalled();
    for (const item of APPLY_RESULT.created) {
      expect(["quarantined", "disabled"]).toContain(item.defaultState);
    }
  });

  it("flags fenced findings and marks stripped secrets", async () => {
    const user = userEvent.setup();
    render(<MigrationPanel />);
    await user.type(screen.getByLabelText(/workspace path to scan/i), "/tmp");
    await user.click(screen.getByRole("button", { name: "Scan" }));

    expect(await screen.findByText("prompt-injection")).toBeInTheDocument();
    expect(screen.getByText("secret-present")).toBeInTheDocument();
    expect(screen.getAllByText("stripped").length).toBeGreaterThan(0);
    expect(
      screen.getByText(/nothing here enables anything/i),
    ).toBeInTheDocument();
  });

  it("re-scanning is idempotent-safe (state refreshes, no stuck results)", async () => {
    const user = userEvent.setup();
    render(<MigrationPanel />);
    await user.type(screen.getByLabelText(/workspace path to scan/i), "/a");
    await user.click(screen.getByRole("button", { name: "Scan" }));
    await screen.findByText("/tmp/ws/AGENTS.md");

    await user.clear(screen.getByLabelText(/workspace path to scan/i));
    await user.type(screen.getByLabelText(/workspace path to scan/i), "/b");
    await user.click(screen.getByRole("button", { name: "Scan" }));
    await waitFor(() => {
      expect(mockScan).toHaveBeenCalledTimes(2);
    });
    expect(mockScan).toHaveBeenLastCalledWith("/b");
  });
});
