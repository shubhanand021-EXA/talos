import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { SandboxPanel } from "./SandboxPanel.js";

const { mockGetStatus, mockGetProfile } = vi.hoisted(() => ({
  mockGetStatus: vi.fn(),
  mockGetProfile: vi.fn(),
}));

vi.mock("../../bridge/index.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../bridge/index.js")>();
  return {
    ...actual,
    getSandboxStatus: mockGetStatus,
    getSandboxProfileForTask: mockGetProfile,
  };
});

const STATUS = {
  platform: "linux",
  failClosed: true,
  backends: [
    {
      name: "bubblewrap",
      available: true,
      enforcementMethod: "bubblewrap+prlimit",
      detail: "bwrap present",
    },
  ],
  checkedAt: "2026-11-02T00:00:00.000Z",
};

describe("SandboxPanel", () => {
  it("renders platform backends and fail-closed badge", async () => {
    mockGetStatus.mockResolvedValueOnce(STATUS);
    render(<SandboxPanel />);
    fireEvent.click(screen.getByRole("button", { name: "Refresh status" }));
    expect(
      await screen.findByTestId("sandbox-backend-bubblewrap"),
    ).toBeTruthy();
    expect(screen.getByText("fail closed")).toBeTruthy();
    expect(screen.getByText("linux")).toBeTruthy();
  });

  it("warns when no backend is available", async () => {
    mockGetStatus.mockResolvedValueOnce({
      ...STATUS,
      backends: [
        {
          name: "bubblewrap",
          available: false,
          enforcementMethod: "unavailable",
          detail: "bwrap missing",
        },
      ],
    });
    render(<SandboxPanel />);
    fireEvent.click(screen.getByRole("button", { name: "Refresh status" }));
    expect(
      await screen.findByText("sandbox unavailable — execution denied"),
    ).toBeTruthy();
  });

  it("resolves and renders per-task profiles", async () => {
    mockGetStatus.mockResolvedValueOnce(STATUS);
    mockGetProfile.mockResolvedValueOnce({
      taskId: "task-1",
      profile: "workspace_write",
      enforced: true,
      enforcementMethod: "bubblewrap+prlimit",
      reason: "workspace-backed task executes confined",
    });
    render(<SandboxPanel />);
    fireEvent.change(screen.getByLabelText("Task id for profile lookup"), {
      target: { value: "task-1" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Resolve profile" }));
    expect(await screen.findByTestId("sandbox-profile-result")).toBeTruthy();
    expect(screen.getByText("workspace_write")).toBeTruthy();
    expect(screen.getByText("enforced")).toBeTruthy();
  });
});
