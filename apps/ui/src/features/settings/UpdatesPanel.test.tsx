import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { UpdatesSection } from "./UpdatesPanel.js";

const { mockCheck, mockInstall } = vi.hoisted(() => ({
  mockCheck: vi.fn(),
  mockInstall: vi.fn(),
}));

vi.mock("../../bridge/index.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../bridge/index.js")>();
  return {
    ...actual,
    checkForUpdates: mockCheck,
    installUpdate: mockInstall,
  };
});

const IDLE = {
  available: false,
  currentVersion: "0.3.0",
  latestVersion: null,
  notes: null,
  pubDate: null,
  downloadUrl: null,
};

beforeEach(() => {
  vi.clearAllMocks();
  mockCheck.mockResolvedValue(IDLE);
  mockInstall.mockResolvedValue(null);
});

describe("UpdatesSection", () => {
  it("renders the up-to-date state", async () => {
    render(<UpdatesSection />);
    expect(await screen.findByText("Up to date")).toBeInTheDocument();
    expect(screen.getByText("0.3.0")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Download/ })).toBeNull();
  });

  it("offers download and install when an update is available", async () => {
    mockCheck.mockResolvedValue({
      ...IDLE,
      available: true,
      latestVersion: "0.4.0",
      notes: "Hardening release",
      pubDate: "2026-11-02T00:00:00Z",
      downloadUrl: "https://updates.talos.dev/x86_64/0.4.0",
    });
    render(<UpdatesSection />);
    expect(await screen.findByText("Update available")).toBeInTheDocument();
    expect(screen.getByText("0.3.0 → 0.4.0")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Download & install" }));
    await waitFor(() => {
      expect(mockInstall).toHaveBeenCalledTimes(1);
    });
    expect(
      await screen.findByText(/restarting into the new version/),
    ).toBeInTheDocument();
  });

  it("rechecks on demand and surfaces errors", async () => {
    render(<UpdatesSection />);
    await screen.findByText("Up to date");
    mockCheck.mockRejectedValueOnce(new Error("endpoint unreachable"));
    fireEvent.click(screen.getByRole("button", { name: "Check for updates" }));
    expect(await screen.findByText("endpoint unreachable")).toBeInTheDocument();
  });
});
