import { describe, expect, it } from "vitest";
import {
  BackupCreatedPayloadSchema,
  BackupInfoSchema,
  BackupRestoredPayloadSchema,
  IntegrityCheckCompletedPayloadSchema,
  RestoreReportSchema,
  StorageInfoSchema,
} from "./storage.js";

const NOW = "2026-11-01T00:00:00.000Z";

describe("storage domain", () => {
  it("parses the storage info snapshot", () => {
    const info = {
      dbPath: "C:\\Users\\talos\\AppData\\Roaming\\com.talos.desktop\\talos.db",
      dbSizeBytes: 123456,
      walSizeBytes: 0,
      schemaVersion: 7,
      backupCount: 2,
      lastBackupAt: NOW,
      integrityValid: true,
      chainChecked: 41,
      chainLastSeq: 41,
      lastCheckedAt: NOW,
    } as const;
    expect(StorageInfoSchema.parse(info)).toEqual(info);
    expect(() =>
      StorageInfoSchema.parse({ ...info, dbSizeBytes: -1 }),
    ).toThrow();
  });

  it("parses backup listings and restore reports", () => {
    const backup = {
      fileName: "talos-20261101T000000Z.db",
      path: "/data/backups/talos-20261101T000000Z.db",
      sizeBytes: 65536,
      createdAt: NOW,
    } as const;
    expect(BackupInfoSchema.parse(backup)).toEqual(backup);

    const report = {
      backupPath: backup.path,
      tablesCopied: ["event_log", "runtime_tasks"],
      chainValid: true,
      chainChecked: 12,
      restoredAt: NOW,
    } as const;
    expect(RestoreReportSchema.parse(report)).toEqual(report);
    expect(() =>
      RestoreReportSchema.parse({ ...report, tablesCopied: "event_log" }),
    ).toThrow();
  });

  it("parses storage event payloads", () => {
    expect(
      BackupCreatedPayloadSchema.parse({
        path: "/data/backups/talos-x.db",
        sizeBytes: 10,
        backupCount: 1,
        checkedAt: NOW,
      }),
    ).toMatchObject({ backupCount: 1 });
    expect(
      BackupRestoredPayloadSchema.parse({
        path: "/data/backups/talos-x.db",
        tablesCopied: ["event_log"],
        chainValid: true,
        chainChecked: 3,
        checkedAt: NOW,
      }),
    ).toMatchObject({ chainValid: true });
    expect(
      IntegrityCheckCompletedPayloadSchema.parse({
        valid: false,
        checked: 7,
        firstBadSeq: 8,
        checkedAt: NOW,
      }),
    ).toMatchObject({ valid: false, firstBadSeq: 8 });
  });
});
