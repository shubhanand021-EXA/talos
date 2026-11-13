import { create } from "zustand";
import type {
  MigrationApplyResult,
  MigrationCategory,
  MigrationPreview,
  MigrationScanResult,
} from "@talos/contracts";
import {
  isBackendUnavailable,
  isTauri,
  migrationApply,
  migrationPreview,
  migrationScan,
  subscribeTalosEvent,
  MigrationImportCompletedPayloadSchema,
  MigrationScanCompletedPayloadSchema,
} from "../bridge/index.js";

interface MigrationState {
  scan: MigrationScanResult | null;
  preview: MigrationPreview | null;
  applyResult: MigrationApplyResult | null;
  busy: boolean;
  error: string | null;
  /** Screen-reader + status announcement for the last completed step. */
  announcement: string | null;
  subscribed: boolean;
  runScan: (workspacePath: string) => Promise<void>;
  runPreview: () => Promise<void>;
  runApply: (categories: MigrationCategory[]) => Promise<void>;
  dismissError: () => void;
  subscribeEvents: () => Promise<void>;
}

function toMessage(error: unknown, fallback: string): string {
  if (isBackendUnavailable(error)) {
    return "Backend unavailable — launch the desktop shell to run migration.";
  }
  return error instanceof Error ? error.message : fallback;
}

export const useMigrationStore = create<MigrationState>()((set, get) => ({
  scan: null,
  preview: null,
  applyResult: null,
  busy: false,
  error: null,
  announcement: null,
  subscribed: false,
  dismissError: () => {
    set({ error: null });
  },
  runScan: async (workspacePath) => {
    set({
      busy: true,
      error: null,
      announcement: null,
      preview: null,
      applyResult: null,
    });
    try {
      const scan = await migrationScan(workspacePath);
      set({
        scan,
        announcement: `Scan complete: ${scan.findings.length} findings in ${scan.workspacePath}.`,
      });
    } catch (error) {
      set({ error: toMessage(error, "Migration scan failed") });
    } finally {
      set({ busy: false });
    }
  },
  runPreview: async () => {
    const { scan } = get();
    if (scan === null) {
      set({ error: "Run a scan before previewing." });
      return;
    }
    set({ busy: true, error: null, announcement: null });
    try {
      const preview = await migrationPreview(scan.importId);
      set({
        preview,
        announcement: `Preview ready: ${preview.items.length} items, all quarantined or disabled.`,
      });
    } catch (error) {
      set({ error: toMessage(error, "Migration preview failed") });
    } finally {
      set({ busy: false });
    }
  },
  runApply: async (categories) => {
    const { scan } = get();
    if (scan === null) {
      set({ error: "Run a scan before applying." });
      return;
    }
    set({ busy: true, error: null, announcement: null });
    try {
      const applyResult = await migrationApply(scan.importId, categories);
      set({
        applyResult,
        announcement: `Import complete: ${applyResult.created.length} artifacts quarantined or disabled. Nothing was enabled.`,
      });
    } catch (error) {
      set({ error: toMessage(error, "Migration import failed") });
    } finally {
      set({ busy: false });
    }
  },
  subscribeEvents: async () => {
    if (get().subscribed || !isTauri()) {
      return;
    }
    try {
      await subscribeTalosEvent(
        "migrationScanCompleted",
        (payload) => {
          set({
            announcement: `Daemon scan finished: ${payload.findingCount} findings (evidence ${payload.evidenceId}).`,
          });
        },
        MigrationScanCompletedPayloadSchema,
      );
      await subscribeTalosEvent(
        "migrationImportCompleted",
        (payload) => {
          set({
            announcement: `Daemon import finished: ${payload.createdCount} artifacts quarantined or disabled (evidence ${payload.evidenceId}).`,
          });
        },
        MigrationImportCompletedPayloadSchema,
      );
      set({ subscribed: true });
    } catch {
      // Event bus unavailable (browser preview): command results still drive
      // the panel; a single failed subscription must not take the UI down.
    }
  },
}));
