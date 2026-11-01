import { useCallback, useEffect, useState } from "react";
import {
  GlassPanel,
  KeyValueList,
  Modal,
  NeonButton,
  SectionHeader,
  Spinner,
  StatusPill,
} from "../../design-system/index.js";
import {
  createBackup,
  getStorageInfo,
  listBackups,
  restoreBackup,
  subscribeTalosEvent,
  BackupCreatedPayloadSchema,
  BackupRestoredPayloadSchema,
  IntegrityCheckCompletedPayloadSchema,
  type BackupInfo,
  type StorageInfo,
} from "../../bridge/index.js";
import { IpcEventName } from "@talos/contracts";

function formatBytes(bytes: number): string {
  if (bytes < 1024) {
    return `${bytes} B`;
  }
  const units = ["KB", "MB", "GB"];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(1)} ${units[unit]}`;
}

export function StorageSection() {
  const [info, setInfo] = useState<StorageInfo | null>(null);
  const [backups, setBackups] = useState<BackupInfo[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [pendingRestore, setPendingRestore] = useState<BackupInfo | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const [storageInfo, backupList] = await Promise.all([
        getStorageInfo(),
        listBackups(),
      ]);
      setInfo(storageInfo);
      setBackups(backupList);
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, []);

  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      try {
        const [storageInfo, backupList] = await Promise.all([
          getStorageInfo(),
          listBackups(),
        ]);
        if (!cancelled) {
          setInfo(storageInfo);
          setBackups(backupList);
          setError(null);
        }
      } catch (err) {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : String(err));
        }
      }
    };
    void load();
    const unlistens: Array<() => void> = [];
    void subscribeTalosEvent(
      IpcEventName.backupCreated,
      () => {
        void refresh();
      },
      BackupCreatedPayloadSchema,
    )
      .then((unlisten) => {
        unlistens.push(unlisten);
      })
      .catch(() => undefined);
    void subscribeTalosEvent(
      IpcEventName.backupRestored,
      () => {
        void refresh();
      },
      BackupRestoredPayloadSchema,
    )
      .then((unlisten) => {
        unlistens.push(unlisten);
      })
      .catch(() => undefined);
    void subscribeTalosEvent(
      IpcEventName.integrityCheckCompleted,
      () => {
        void refresh();
      },
      IntegrityCheckCompletedPayloadSchema,
    )
      .then((unlisten) => {
        unlistens.push(unlisten);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
      for (const unlisten of unlistens) {
        unlisten();
      }
    };
  }, [refresh]);

  const onCreateBackup = useCallback(async () => {
    setBusy(true);
    setNotice(null);
    try {
      const backup = await createBackup();
      setNotice(`Backup created: ${backup.fileName}`);
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }, [refresh]);

  const onConfirmRestore = useCallback(async () => {
    if (pendingRestore === null) {
      return;
    }
    setBusy(true);
    setNotice(null);
    try {
      const report = await restoreBackup(pendingRestore.path);
      setNotice(
        report.chainValid
          ? `Restored from ${pendingRestore.fileName} (${report.tablesCopied.length} tables, chain valid)`
          : `Restored from ${pendingRestore.fileName} but the chain is INVALID`,
      );
      setPendingRestore(null);
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }, [pendingRestore, refresh]);

  return (
    <GlassPanel label="Storage">
      <SectionHeader
        title="Durable storage"
        subtitle="File-backed evidence database"
      />
      {info === null && error === null ? (
        <Spinner label="Loading storage info" />
      ) : null}
      {error !== null ? (
        <p className="text-sm text-red-300" role="alert">
          {error}
        </p>
      ) : null}
      {info !== null ? (
        <div className="flex flex-col gap-4">
          <KeyValueList
            entries={[
              { key: "database", value: info.dbPath },
              { key: "size", value: formatBytes(info.dbSizeBytes) },
              { key: "wal", value: formatBytes(info.walSizeBytes) },
              { key: "schema", value: String(info.schemaVersion) },
              { key: "backups", value: String(info.backupCount) },
              {
                key: "last backup",
                value: info.lastBackupAt ?? "never",
              },
              { key: "last check", value: info.lastCheckedAt },
            ]}
          />
          <div className="flex items-center gap-2">
            <span className="text-sm text-slate-400">Integrity</span>
            <StatusPill tone={info.integrityValid ? "ok" : "error"}>
              {info.integrityValid
                ? `valid (${info.chainChecked} links)`
                : "BROKEN — diagnose before writing"}
            </StatusPill>
          </div>
          <div className="flex items-center gap-2">
            <NeonButton
              variant="primary"
              onClick={() => {
                void onCreateBackup();
              }}
            >
              {busy ? "Working…" : "Create backup"}
            </NeonButton>
            <NeonButton
              variant="ghost"
              onClick={() => {
                void refresh();
              }}
            >
              Refresh
            </NeonButton>
          </div>
          {notice !== null ? (
            <p className="text-sm text-emerald-300" role="status">
              {notice}
            </p>
          ) : null}
          <div className="flex flex-col gap-2">
            <h3 className="text-sm font-semibold text-slate-200">
              Backups ({backups.length})
            </h3>
            {backups.length === 0 ? (
              <p className="text-sm text-slate-400">No backups yet.</p>
            ) : (
              <ul className="flex flex-col gap-2">
                {backups.map((backup) => (
                  <li
                    key={backup.path}
                    className="flex items-center justify-between gap-3 rounded-md border border-white/10 px-3 py-2"
                  >
                    <div className="min-w-0">
                      <p className="truncate font-mono text-xs text-slate-200">
                        {backup.fileName}
                      </p>
                      <p className="text-xs text-slate-400">
                        {formatBytes(backup.sizeBytes)} · {backup.createdAt}
                      </p>
                    </div>
                    <NeonButton
                      variant="ghost"
                      onClick={() => {
                        setPendingRestore(backup);
                      }}
                    >
                      Restore
                    </NeonButton>
                  </li>
                ))}
              </ul>
            )}
          </div>
        </div>
      ) : null}
      {pendingRestore !== null ? (
        <Modal
          title="Restore backup"
          onClose={() => {
            setPendingRestore(null);
          }}
        >
          <p className="text-sm text-slate-300">
            Replace the live database with{" "}
            <span className="font-mono">{pendingRestore.fileName}</span>?
            Current content is replaced inside one transaction and the hash
            chain is verified after the copy.
          </p>
          <div className="mt-4 flex justify-end gap-2">
            <NeonButton
              variant="ghost"
              onClick={() => {
                setPendingRestore(null);
              }}
            >
              Cancel
            </NeonButton>
            <NeonButton
              variant="primary"
              onClick={() => {
                void onConfirmRestore();
              }}
            >
              {busy ? "Restoring…" : "Confirm restore"}
            </NeonButton>
          </div>
        </Modal>
      ) : null}
    </GlassPanel>
  );
}
