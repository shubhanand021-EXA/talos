import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { VmViewer } from "./VmViewer.js";

const { mockStatus, mockFrame, mockLaunch, mockDestroy } = vi.hoisted(() => ({
  mockStatus: vi.fn(),
  mockFrame: vi.fn(),
  mockLaunch: vi.fn(),
  mockDestroy: vi.fn(),
}));

vi.mock("../../bridge/index.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../bridge/index.js")>();
  return {
    ...actual,
    computerVmStatus: mockStatus,
    computerVmFrame: mockFrame,
    launchComputerVm: mockLaunch,
    destroyComputerVm: mockDestroy,
  };
});

const STOPPED = {
  running: false,
  containerId: null,
  containerName: null,
  vncUrl: null,
  width: 1280,
  height: 800,
  startedAt: null,
  backend: "ephemeral-vm",
};

beforeEach(() => {
  mockStatus.mockReset();
  mockFrame.mockReset();
  mockLaunch.mockReset();
  mockDestroy.mockReset();
  mockStatus.mockResolvedValue(STOPPED);
  mockFrame.mockRejectedValue(new Error("vm_not_running"));
  mockLaunch.mockResolvedValue({
    ...STOPPED,
    running: true,
    containerId: "mock-0001",
    containerName: "talos-vm-task-1",
  });
  mockDestroy.mockResolvedValue(true);
});

describe("VmViewer", () => {
  it("renders the idle state and launches a container", async () => {
    const user = userEvent.setup();
    render(<VmViewer taskId="task-1" />);
    expect(await screen.findByText("no container")).toBeInTheDocument();
    expect(
      screen.getByText(/The host screen is never touched/),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Launch VM" }));
    expect(mockLaunch).toHaveBeenCalledWith("task-1");
    expect(await screen.findByText("container running")).toBeInTheDocument();
  });

  it("streams frames with seq and hash readout", async () => {
    mockFrame.mockResolvedValue({
      seq: 3,
      hash: "sha256:abc",
      width: 1280,
      height: 800,
      pngBase64: "iVBORw0KGgo=",
      at: "2026-11-08T00:00:00.000Z",
    });
    render(<VmViewer taskId="task-1" />);
    expect(
      await screen.findByAltText("Ephemeral VM desktop"),
    ).toBeInTheDocument();
    expect(screen.getByText(/seq 3/)).toBeInTheDocument();
  });

  it("destroys the container and clears the frame", async () => {
    const user = userEvent.setup();
    render(<VmViewer taskId="task-1" />);
    expect(await screen.findByText("no container")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Destroy VM" }));
    expect(mockDestroy).toHaveBeenCalled();
  });

  it("surfaces launch failures without crashing", async () => {
    const user = userEvent.setup();
    mockLaunch.mockRejectedValue(new Error("vm_unavailable: no daemon"));
    render(<VmViewer taskId="task-1" />);
    expect(await screen.findByText("no container")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Launch VM" }));
    expect(
      await screen.findByText("vm_unavailable: no daemon"),
    ).toBeInTheDocument();
  });
});
