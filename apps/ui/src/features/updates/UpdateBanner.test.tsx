import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { UpdateBanner } from "./UpdateBanner.js";

const { mockStatus, mockDownload, mockInstall } = vi.hoisted(() => ({
  mockStatus: vi.fn(),
  mockDownload: vi.fn(),
  mockInstall: vi.fn(),
}));

vi.mock("../../bridge/index.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../bridge/index.js")>();
  return {
    ...actual,
    isTauri: () => true,
    getUpdateStatus: mockStatus,
    downloadUpdate: mockDownload,
    installUpdate: mockInstall,
  };
});

const IDLE = {
  available: false,
  currentVersion: "0.4.0",
  latestVersion: null,
  notes: null,
  pubDate: null,
  downloadUrl: null,
  channel: "stable",
  rolloutApplicable: true,
  staged: false,
  stagedVersion: null,
  stagedDownloadedAt: null,
};

beforeEach(() => {
  vi.clearAllMocks();
});

describe("UpdateBanner", () => {
  it("renders nothing when up to date", async () => {
    mockStatus.mockResolvedValue(IDLE);
    const { container } = render(<UpdateBanner />);
    await waitFor(() => {
      expect(mockStatus).toHaveBeenCalledTimes(1);
    });
    expect(container).toBeEmptyDOMElement();
  });

  it("renders nothing when the backend is unreachable", async () => {
    mockStatus.mockRejectedValueOnce(new Error("offline"));
    const { container } = render(<UpdateBanner />);
    await waitFor(() => {
      expect(mockStatus).toHaveBeenCalledTimes(1);
    });
    expect(container).toBeEmptyDOMElement();
  });

  it("offers staged download then install and restart", async () => {
    mockStatus.mockResolvedValue({
      ...IDLE,
      available: true,
      latestVersion: "0.5.0",
    });
    mockDownload.mockResolvedValue({
      ...IDLE,
      available: true,
      latestVersion: "0.5.0",
      staged: true,
      stagedVersion: "0.5.0",
    });
    mockInstall.mockResolvedValue(null);
    render(<UpdateBanner />);
    expect(await screen.findByText("Update available")).toBeInTheDocument();
    expect(screen.getByText("0.4.0 → 0.5.0")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Download" }));
    await waitFor(() => {
      expect(mockDownload).toHaveBeenCalledTimes(1);
    });
    expect(
      await screen.findByRole("button", { name: "Install & restart" }),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Install & restart" }));
    await waitFor(() => {
      expect(mockInstall).toHaveBeenCalledTimes(1);
    });
    expect(
      await screen.findByText(/Update installed — restarting/),
    ).toBeInTheDocument();
  });

  it("installs staged bytes directly without re-downloading", async () => {
    mockStatus.mockResolvedValue({
      ...IDLE,
      available: true,
      latestVersion: "0.5.0",
      staged: true,
      stagedVersion: "0.5.0",
    });
    mockInstall.mockResolvedValue(null);
    render(<UpdateBanner />);
    expect(
      await screen.findByRole("button", { name: "Install & restart" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Download" }),
    ).not.toBeInTheDocument();
  });

  it("surfaces download errors inline", async () => {
    mockStatus.mockResolvedValue({
      ...IDLE,
      available: true,
      latestVersion: "0.5.0",
    });
    mockDownload.mockRejectedValueOnce(new Error("network down"));
    render(<UpdateBanner />);
    expect(await screen.findByText("Update available")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Download" }));
    expect(await screen.findByText("network down")).toBeInTheDocument();
  });
});
