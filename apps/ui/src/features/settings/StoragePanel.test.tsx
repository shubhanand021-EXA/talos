import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { StorageSection } from "./StoragePanel.js";

const { mockGetInfo, mockList, mockCreate, mockRestore } = vi.hoisted(() => ({
  mockGetInfo: vi.fn(),
  mockList: vi.fn(),
  mockCreate: vi.fn(),
  mockRestore: vi.fn(),
}));

vi.mock("../../bridge/index.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../bridge/index.js")>();
  return {
    ...actual,
    getStorageInfo: mockGetInfo,
    listBackups: mockList,
    createBackup: mockCreate,
    restoreBackup: mockRestore,
    subscribeTalosEvent: vi.fn().mockResolvedValue(() => undefined),
  };
});

const INFO = {
  dbPath: "C:\\data\\com.talos.desktop\\talos.db",
  dbSizeBytes: 123456,
  walSizeBytes: 0,
  schemaVersion: 7,
  backupCount: 1,
  lastBackupAt: "2026-11-01T00:00:00.000Z",
  integrityValid: true,
  chainChecked: 41,
  chainLastSeq: 41,
  lastCheckedAt: "2026-11-01T00:00:00.000Z",
};

const BACKUPS = [
  {
    fileName: "talos-20261101T000000Z.db",
    path: "C:\\data\\backups\\talos-20261101T000000Z.db",
    sizeBytes: 65536,
    createdAt: "2026-11-01T00:00:00.000Z",
  },
];

beforeEach(() => {
  vi.clearAllMocks();
  mockGetInfo.mockResolvedValue(INFO);
  mockList.mockResolvedValue(BACKUPS);
  mockCreate.mockResolvedValue(BACKUPS[0]);
  mockRestore.mockResolvedValue({
    backupPath: BACKUPS[0]?.path,
    tablesCopied: ["event_log"],
    chainValid: true,
    chainChecked: 41,
    restoredAt: "2026-11-01T00:01:00.000Z",
  });
});

describe("StorageSection", () => {
  it("renders database info and integrity status", async () => {
    render(<StorageSection />);
    expect(await screen.findByText("Durable storage")).toBeTruthy();
    expect(await screen.findByText(/talos\.db/)).toBeTruthy();
    expect(await screen.findByText(/valid \(41 links\)/)).toBeTruthy();
    expect(await screen.findByText("talos-20261101T000000Z.db")).toBeTruthy();
  });

  it("surfaces a broken chain honestly", async () => {
    mockGetInfo.mockResolvedValue({ ...INFO, integrityValid: false });
    render(<StorageSection />);
    expect(await screen.findByText(/BROKEN/)).toBeTruthy();
  });

  it("creates backups through IPC", async () => {
    render(<StorageSection />);
    fireEvent.click(await screen.findByText("Create backup"));
    await waitFor(() => {
      expect(mockCreate).toHaveBeenCalledTimes(1);
    });
    expect(await screen.findByText(/Backup created/)).toBeTruthy();
  });

  it("restores behind a confirmation dialog", async () => {
    render(<StorageSection />);
    fireEvent.click(await screen.findByText("Restore"));
    expect(await screen.findByText("Restore backup")).toBeTruthy();
    fireEvent.click(await screen.findByText("Confirm restore"));
    await waitFor(() => {
      expect(mockRestore).toHaveBeenCalledWith(BACKUPS[0]?.path);
    });
    expect(await screen.findByText(/Restored from/)).toBeTruthy();
  });
});
