import { z } from "zod";

/**
 * Migration kit contracts (Prompt 011): quarantined import of competitor
 * instruction / skill / heartbeat / MCP artifacts. Imported content is
 * untrusted DATA, never authority: everything lands quarantined/disabled
 * and enabling is a separate approval.
 */

export const MigrationCategorySchema = z.enum([
  "instructions",
  "skills",
  "schedules",
  "mcp-servers",
]);
export type MigrationCategory = z.infer<typeof MigrationCategorySchema>;

export const MigrationFindingSchema = z
  .object({
    sourcePath: z.string().min(1).max(1024),
    kind: z.enum([
      "agents",
      "claude",
      "skill",
      "heartbeat",
      "mcp-config",
      "unknown",
    ]),
    category: MigrationCategorySchema,
    flagged: z.boolean(),
    riskLabels: z.array(z.string().min(1).max(64)).max(16),
    secretStripped: z.boolean(),
    note: z.string().max(1024),
  })
  .strict();
export type MigrationFinding = z.infer<typeof MigrationFindingSchema>;

export const MigrationScanResultSchema = z
  .object({
    importId: z.string().min(1).max(128),
    workspacePath: z.string().min(1).max(1024),
    findings: z.array(MigrationFindingSchema).max(64),
    scannedAt: z.string().min(1),
  })
  .strict();
export type MigrationScanResult = z.infer<typeof MigrationScanResultSchema>;

export const MigrationPreviewItemSchema = z
  .object({
    category: MigrationCategorySchema,
    action: z.string().min(1).max(128),
    target: z.string().min(1).max(256),
    defaultState: z.enum(["quarantined", "disabled"]),
    riskLabels: z.array(z.string().min(1).max(64)).max(16),
    secretStripped: z.boolean(),
  })
  .strict();
export type MigrationPreviewItem = z.infer<typeof MigrationPreviewItemSchema>;

export const MigrationPreviewSchema = z
  .object({
    importId: z.string().min(1).max(128),
    items: z.array(MigrationPreviewItemSchema).max(128),
    previewedAt: z.string().min(1),
  })
  .strict();
export type MigrationPreview = z.infer<typeof MigrationPreviewSchema>;

export const MigrationApplyResultSchema = z
  .object({
    importId: z.string().min(1).max(128),
    approvedCategories: z.array(MigrationCategorySchema).max(4),
    created: z.array(MigrationPreviewItemSchema).max(128),
    skipped: z.array(MigrationPreviewItemSchema).max(128),
    evidenceIds: z.array(z.string().min(1).max(128)).max(16),
    appliedAt: z.string().min(1),
  })
  .strict();
export type MigrationApplyResult = z.infer<typeof MigrationApplyResultSchema>;

export const MigrationScanInputSchema = z
  .object({ workspacePath: z.string().min(1).max(1024) })
  .strict();
export type MigrationScanInput = z.infer<typeof MigrationScanInputSchema>;

export const MigrationPreviewInputSchema = z
  .object({ importId: z.string().min(1).max(128) })
  .strict();
export type MigrationPreviewInput = z.infer<typeof MigrationPreviewInputSchema>;

export const MigrationApplyInputSchema = z
  .object({
    importId: z.string().min(1).max(128),
    approvedCategories: z.array(MigrationCategorySchema).min(1).max(4),
  })
  .strict();
export type MigrationApplyInput = z.infer<typeof MigrationApplyInputSchema>;
