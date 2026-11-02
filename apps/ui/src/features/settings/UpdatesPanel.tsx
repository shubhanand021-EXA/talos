import { useCallback, useEffect, useState } from "react";
import {
  GlassPanel,
  KeyValueList,
  NeonButton,
  SectionHeader,
  Spinner,
  StatusPill,
} from "../../design-system/index.js";
import {
  checkForUpdates,
  getUpdateChannel,
  installUpdate,
  setUpdateChannel,
  type UpdateInfo,
} from "../../bridge/index.js";

const CHANNELS = ["stable", "beta", "nightly"] as const;

export function UpdatesSection() {
  const [info, setInfo] = useState<UpdateInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [checking, setChecking] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [channel, setChannel] = useState<string>("stable");
  const [channelBusy, setChannelBusy] = useState(false);

  const refresh = useCallback(async () => {
    setChecking(true);
    setNotice(null);
    try {
      setInfo(await checkForUpdates());
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setChecking(false);
    }
  }, []);

  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      try {
        const [status, nextChannel] = await Promise.all([
          checkForUpdates(),
          getUpdateChannel().catch(() => "stable"),
        ]);
        if (!cancelled) {
          setInfo(status);
          setChannel(nextChannel);
          setError(null);
        }
      } catch (err) {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : String(err));
        }
      }
    };
    void load();
    return () => {
      cancelled = true;
    };
  }, []);

  const onInstall = useCallback(async () => {
    setInstalling(true);
    setNotice(null);
    try {
      await installUpdate();
      setNotice("Update installed — restarting into the new version…");
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setInstalling(false);
    }
  }, []);

  return (
    <GlassPanel label="Updates">
      <SectionHeader
        title="Signed updates"
        subtitle="Staged rollouts · ed25519-verified before install"
      />
      {info === null && error === null ? (
        <Spinner label="Loading update status" />
      ) : null}
      {error !== null ? (
        <p className="text-sm text-red-300" role="alert">
          {error}
        </p>
      ) : null}
      {info !== null ? (
        <div className="flex flex-col gap-4">
          <div className="flex items-center gap-2">
            <StatusPill tone={info.available ? "ok" : "neutral"}>
              {info.available ? "Update available" : "Up to date"}
            </StatusPill>
            {info.available && info.latestVersion !== null ? (
              <span className="text-sm text-slate-300">
                {info.currentVersion} → {info.latestVersion}
              </span>
            ) : (
              <span className="text-sm text-slate-300">
                {info.currentVersion}
              </span>
            )}
          </div>
          <KeyValueList
            entries={[
              { key: "channel", value: `${channel} (staged rollout)` },
              {
                key: "signature",
                value: "ed25519, verified before install",
              },
              ...(info.available && info.latestVersion
                ? [
                    { key: "latest", value: info.latestVersion },
                    {
                      key: "published",
                      value: info.pubDate ?? "unknown",
                    },
                    {
                      key: "download",
                      value: info.downloadUrl ?? "via update endpoint",
                    },
                  ]
                : []),
            ]}
          />
          {info.notes !== null &&
          info.notes !== undefined &&
          info.notes !== "" ? (
            <p className="text-sm text-slate-300">{info.notes}</p>
          ) : null}
          <div
            role="radiogroup"
            aria-label="Update channel"
            className="flex items-center gap-2"
          >
            <span className="text-xs text-slate-500">Channel:</span>
            {CHANNELS.map((name) => (
              <NeonButton
                key={name}
                variant={channel === name ? "primary" : "ghost"}
                aria-pressed={channel === name}
                disabled={channelBusy}
                onClick={() => {
                  setChannelBusy(true);
                  setNotice(null);
                  void setUpdateChannel(name)
                    .then((next) => {
                      setChannel(next);
                      setNotice(
                        `Channel: ${next}. Beta/nightly builds ship their own endpoint; restart to apply.`,
                      );
                    })
                    .catch((err: unknown) => {
                      setError(
                        err instanceof Error ? err.message : String(err),
                      );
                    })
                    .finally(() => {
                      setChannelBusy(false);
                    });
                }}
              >
                {name}
              </NeonButton>
            ))}
          </div>
          <div className="flex items-center gap-2">
            <NeonButton variant="ghost" onClick={() => void refresh()}>
              {checking ? "Checking…" : "Check for updates"}
            </NeonButton>
            {info.available ? (
              <NeonButton variant="primary" onClick={() => void onInstall()}>
                {installing ? "Installing…" : "Download & install"}
              </NeonButton>
            ) : null}
          </div>
          {notice !== null ? (
            <p className="text-sm text-emerald-300" role="status">
              {notice}
            </p>
          ) : null}
          <p className="text-xs text-slate-500">
            Releases are signed offline with scripts/sign-release.mjs. The
            private key never enters the repository; the bundled public key pins
            which signatures this app trusts.
          </p>
        </div>
      ) : null}
    </GlassPanel>
  );
}
