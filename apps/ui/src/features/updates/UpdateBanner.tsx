import { useCallback, useEffect, useState } from "react";
import { NeonButton, StatusPill } from "../../design-system/index.js";
import {
  downloadUpdate,
  getUpdateStatus,
  installUpdate,
  isTauri,
  type UpdateStatusView,
} from "../../bridge/index.js";

/**
 * Update notification banner for the TopBar. Polls the staged-rollout
 * status once on mount (never blocks the shell), then offers staged
 * download → install → restart without leaving the current view.
 */
export function UpdateBanner() {
  const [status, setStatus] = useState<UpdateStatusView | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [installed, setInstalled] = useState(false);

  useEffect(() => {
    if (!isTauri()) {
      return;
    }
    let cancelled = false;
    void getUpdateStatus()
      .then((next) => {
        if (!cancelled) {
          setStatus(next);
        }
      })
      .catch(() => {
        // Update endpoint unreachable: stay silent (Settings → Updates
        // surfaces the error on demand instead of nagging here).
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const onDownload = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      setStatus(await downloadUpdate());
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }, []);

  const onInstall = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      await installUpdate();
      setInstalled(true);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }, []);

  if (status === null || !status.available) {
    return null;
  }
  if (installed) {
    return (
      <p className="text-xs text-emerald-300" role="status">
        Update installed — restarting…
      </p>
    );
  }
  return (
    <div
      role="status"
      aria-label="Update available"
      className="flex items-center gap-2 rounded-control border border-amber-300/30 bg-amber-400/10 px-2 py-1"
    >
      <StatusPill tone="warn">Update available</StatusPill>
      <span className="font-mono text-xs text-slate-200">
        {status.currentVersion} →{" "}
        {status.stagedVersion ?? status.latestVersion ?? "latest"}
        {status.channel !== "stable" ? ` (${status.channel})` : ""}
      </span>
      {status.staged ? (
        <NeonButton
          variant="primary"
          disabled={busy}
          onClick={() => {
            void onInstall();
          }}
        >
          {busy ? "Installing…" : "Install & restart"}
        </NeonButton>
      ) : (
        <NeonButton
          variant="primary"
          disabled={busy}
          onClick={() => {
            void onDownload();
          }}
        >
          {busy ? "Downloading…" : "Download"}
        </NeonButton>
      )}
      {error !== null ? (
        <span className="text-xs text-red-300">{error}</span>
      ) : null}
    </div>
  );
}
