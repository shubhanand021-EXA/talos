import { z } from "zod";

/** Health snapshot of the durable file-backed store. */
export const StorageInfoSchema = z
  .object({
    dbPath: z.string().min(1).max(1024),
    dbSizeBytes: z.number().int().nonnegative(),
    walSizeBytes: z.number().int().nonnegative(),
    schemaVersion: z.number().int().nonnegative(),
    backupCount: z.number().int().nonnegative(),
    lastBackupAt: z.string().min(1).nullish(),
    integrityValid: z.boolean(),
    chainChecked: z.number().int().nonnegative(),
    chainLastSeq: z.number().int().nonnegative().nullish(),
    lastCheckedAt: z.string().min(1),
  })
  .strict();
export type StorageInfo = z.infer<typeof StorageInfoSchema>;

/** One timestamped backup snapshot. */
export const BackupInfoSchema = z
  .object({
    fileName: z.string().min(1).max(256),
    path: z.string().min(1).max(1024),
    sizeBytes: z.number().int().nonnegative(),
    createdAt: z.string().min(1),
  })
  .strict();
export type BackupInfo = z.infer<typeof BackupInfoSchema>;

/** Outcome of restoring a backup snapshot. */
export const RestoreReportSchema = z
  .object({
    backupPath: z.string().min(1).max(1024),
    tablesCopied: z.array(z.string().min(1).max(128)).max(64),
    chainValid: z.boolean(),
    chainChecked: z.number().int().nonnegative(),
    restoredAt: z.string().min(1),
  })
  .strict();
export type RestoreReport = z.infer<typeof RestoreReportSchema>;

/** Payload of `backupCreated`: snapshot linkage, never content. */
export const BackupCreatedPayloadSchema = z
  .object({
    path: z.string().min(1).max(1024),
    sizeBytes: z.number().int().nonnegative(),
    backupCount: z.number().int().nonnegative(),
    checkedAt: z.string().min(1),
  })
  .strict();
export type BackupCreatedPayload = z.infer<typeof BackupCreatedPayloadSchema>;

/** Payload of `backupRestored`: restore linkage + chain verdict. */
export const BackupRestoredPayloadSchema = z
  .object({
    path: z.string().min(1).max(1024),
    tablesCopied: z.array(z.string().min(1).max(128)).max(64),
    chainValid: z.boolean(),
    chainChecked: z.number().int().nonnegative(),
    checkedAt: z.string().min(1),
  })
  .strict();
export type BackupRestoredPayload = z.infer<typeof BackupRestoredPayloadSchema>;

/** Payload of `integrityCheckCompleted`: chain verdict, never content. */
export const IntegrityCheckCompletedPayloadSchema = z
  .object({
    valid: z.boolean(),
    checked: z.number().int().nonnegative(),
    firstBadSeq: z.number().int().nonnegative().nullish(),
    checkedAt: z.string().min(1),
  })
  .strict();
export type IntegrityCheckCompletedPayload = z.infer<
  typeof IntegrityCheckCompletedPayloadSchema
>;
