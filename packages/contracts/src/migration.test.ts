import { describe, expect, it } from "vitest";
import {
  MigrationApplyInputSchema,
  MigrationPreviewSchema,
  MigrationScanResultSchema,
} from "./migration.js";

const FINDING = {
  sourcePath: "AGENTS.md",
  kind: "agents",
  category: "instructions",
  flagged: false,
  riskLabels: [],
  secretStripped: false,
  note: "clean; import quarantined/disabled",
} as const;

describe("migration contracts", () => {
  it("scan result round-trips", () => {
    const scan = {
      importId: "migration-abc123",
      workspacePath: "/tmp/ws",
      findings: [FINDING],
      scannedAt: "2026-11-13T00:00:00.000Z",
    };
    expect(MigrationScanResultSchema.parse(scan)).toEqual(scan);
  });

  it("preview items land quarantined or disabled only", () => {
    const preview = {
      importId: "migration-abc123",
      items: [
        {
          category: "mcp-servers",
          action: "stage-mcp-server",
          target: "mcp-server:.mcp.json",
          defaultState: "disabled",
          riskLabels: ["secret-present"],
          secretStripped: true,
        },
      ],
      previewedAt: "2026-11-13T00:00:00.000Z",
    };
    const parsed = MigrationPreviewSchema.parse(preview);
    for (const item of parsed.items) {
      expect(["quarantined", "disabled"]).toContain(item.defaultState);
    }
  });

  it("apply input requires at least one approved category", () => {
    expect(() =>
      MigrationApplyInputSchema.parse({
        importId: "migration-abc123",
        approvedCategories: [],
      }),
    ).toThrow();
    expect(
      MigrationApplyInputSchema.parse({
        importId: "migration-abc123",
        approvedCategories: ["skills"],
      }),
    ).toBeTruthy();
  });
});
