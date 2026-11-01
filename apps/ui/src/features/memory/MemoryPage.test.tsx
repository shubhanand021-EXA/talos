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

import { MemoryPage } from "./MemoryPage.js";
import { useAppStore } from "../../stores/appStore.js";

const STATUS = {
  entries: 3,
  bySourceType: [
    { sourceType: "claim", count: 1 },
    { sourceType: "evidence", count: 2 },
  ],
  backend: "local-hash-384/1",
  dims: 384,
  modelLoaded: false,
  modelDir: ".talos/models",
  warning: "no ONNX runtime bundled",
  lastIndexedAt: "2026-10-31T00:00:00.000Z",
};

const HITS = [
  {
    id: "claim:claim-1",
    sourceType: "claim",
    sourceId: "claim-1",
    taskId: "task-1",
    score: 0.87,
    lexicalScore: 0.75,
    semanticScore: 0.92,
    snippet: "reads succeed with hash",
    verified: true,
    createdAt: "2026-10-31T00:00:00.000Z",
    reason: "hybrid blend (lexical 0.75, semantic 0.92); ref claim:claim-1",
  },
];

function renderPage() {
  return render(
    <MemoryRouter>
      <MemoryPage />
    </MemoryRouter>,
  );
}

beforeEach(() => {
  vi.unstubAllGlobals();
  mockInvoke.mockReset();
  useAppStore.setState({ liveFeed: [], evidenceVersion: 0 });
});

describe("MemoryPage", () => {
  it("renders the mock snapshot outside Tauri", () => {
    renderPage();
    expect(
      screen.getByText("mock snapshot", { exact: false }),
    ).toBeInTheDocument();
    expect(
      screen.getByText("local-hash-384/1", { exact: false }),
    ).toBeInTheDocument();
    expect(mockInvoke).not.toHaveBeenCalled();
  });

  it("renders index status with entries and backend", async () => {
    vi.stubGlobal("__TAURI_INTERNALS__", {});
    mockInvoke.mockImplementation((command: string) => {
      if (command === "get_index_status") {
        return Promise.resolve(STATUS);
      }
      if (command === "list_skills") {
        return Promise.resolve([]);
      }
      return Promise.reject(new Error(`unexpected ${command}`));
    });
    renderPage();
    expect(
      await screen.findByText("3 entries", { exact: false }),
    ).toBeInTheDocument();
    expect(
      screen.getByText("local-hash-384/1", { exact: false }),
    ).toBeInTheDocument();
  });

  it("searches memory and renders scored results", async () => {
    vi.stubGlobal("__TAURI_INTERNALS__", {});
    mockInvoke.mockImplementation((command: string) => {
      if (command === "get_index_status") {
        return Promise.resolve(STATUS);
      }
      if (command === "list_skills") {
        return Promise.resolve([]);
      }
      if (command === "search_memory") {
        return Promise.resolve(HITS);
      }
      return Promise.reject(new Error(`unexpected ${command}`));
    });
    const user = userEvent.setup();
    renderPage();
    await user.click(screen.getByRole("button", { name: "Search memory" }));
    expect(
      await screen.findByText("reads succeed with hash"),
    ).toBeInTheDocument();
    expect(screen.getByText("0.87")).toBeInTheDocument();
    expect(screen.getByText("verified")).toBeInTheDocument();
  });

  it("surfaces backend failures without hiding the panel", async () => {
    vi.stubGlobal("__TAURI_INTERNALS__", {});
    mockInvoke.mockRejectedValue(new Error("memory offline"));
    renderPage();
    expect(await screen.findByText("No index status")).toBeInTheDocument();
    expect(await screen.findByText("memory offline")).toBeInTheDocument();
  });

  it("rebuilds the index and refreshes status and skills", async () => {
    vi.stubGlobal("__TAURI_INTERNALS__", {});
    mockInvoke.mockImplementation((command: string) => {
      if (command === "get_index_status" || command === "rebuild_index") {
        return Promise.resolve(STATUS);
      }
      if (command === "list_skills") {
        return Promise.resolve([
          {
            id: "skill-task-1",
            name: "read then hash",
            description: "Replay of task-1.",
            version: "0.1.0",
            status: "quarantined",
            sourceTaskId: "task-1",
            permissionCeiling: "low",
            requiredTools: ["fs.read"],
            createdAt: "2026-10-31T00:00:00.000Z",
          },
        ]);
      }
      return Promise.reject(new Error(`unexpected ${command}`));
    });
    const user = userEvent.setup();
    renderPage();
    await user.click(
      await screen.findByRole("button", { name: "Rebuild Index" }),
    );
    await waitFor(() => {
      expect(screen.getByText("read then hash")).toBeInTheDocument();
    });
    expect(screen.getByText("quarantined")).toBeInTheDocument();
  });
});
