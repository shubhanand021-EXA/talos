import { useCallback, useEffect, useState } from "react";
import type { ChannelStatusView, LoggedMessage } from "@talos/contracts";
import { isTauri } from "../../bridge/client.js";
import {
  linkChannel,
  listChannelMessages,
  listChannels,
  unlinkChannel,
} from "../../bridge/gateway.js";
import {
  GlassPanel,
  NeonButton,
  SectionHeader,
  Spinner,
  StatusPill,
} from "../../design-system/index.js";

/**
 * Channels panel (Settings): link/unlink Telegram + Discord bots, view
 * linked state, and inspect the recent inbound/outbound message log.
 * Tokens travel once into the OS keychain and are never displayed back.
 */
export function ChannelsPanel() {
  const [channels, setChannels] = useState<ChannelStatusView[] | null>(null);
  const [messages, setMessages] = useState<LoggedMessage[]>([]);
  const [tokens, setTokens] = useState<Record<string, string>>({});
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    if (!isTauri()) {
      setError("Channels live in the Tauri shell — launch the desktop app.");
      setChannels([]);
      return;
    }
    try {
      const [rows, log] = await Promise.all([
        listChannels(),
        listChannelMessages(20),
      ]);
      setChannels(rows);
      setMessages(log);
      setError(null);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Channels unreachable");
      setChannels([]);
    }
  }, []);

  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      if (!isTauri()) {
        if (!cancelled) {
          setError(
            "Channels live in the Tauri shell — launch the desktop app.",
          );
          setChannels([]);
        }
        return;
      }
      try {
        const [rows, log] = await Promise.all([
          listChannels(),
          listChannelMessages(20),
        ]);
        if (!cancelled) {
          setChannels(rows);
          setMessages(log);
          setError(null);
        }
      } catch (cause) {
        if (!cancelled) {
          setError(
            cause instanceof Error ? cause.message : "Channels unreachable",
          );
          setChannels([]);
        }
      }
    };
    void load();
    return () => {
      cancelled = true;
    };
  }, []);

  const link = useCallback(
    async (channel: string) => {
      const token = (tokens[channel] ?? "").trim();
      if (token === "") {
        setError(`Paste a ${channel} token or webhook URL first.`);
        return;
      }
      setBusy(channel);
      try {
        const rows = await linkChannel(channel, token);
        setChannels(rows);
        setTokens((prev) => ({ ...prev, [channel]: "" }));
        setError(null);
      } catch (cause) {
        setError(cause instanceof Error ? cause.message : "Link failed");
      } finally {
        setBusy(null);
      }
    },
    [tokens],
  );

  const unlink = useCallback(async (channel: string) => {
    setBusy(channel);
    try {
      const rows = await unlinkChannel(channel);
      setChannels(rows);
      setError(null);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Unlink failed");
    } finally {
      setBusy(null);
    }
  }, []);

  return (
    <GlassPanel label="Channels">
      <SectionHeader
        title="Messaging channels"
        subtitle="Command Talos from Telegram or Discord; every message passes the policy broker"
      />
      {channels === null ? (
        <Spinner label="Loading channels" />
      ) : channels.length === 0 ? (
        <p className="text-sm text-slate-400" role="status">
          {error ?? "No channels configured."}
        </p>
      ) : (
        <ul className="flex flex-col gap-2">
          {channels.map((row) => (
            <li
              key={row.channel}
              className="flex flex-wrap items-center justify-between gap-3 rounded-md border border-white/10 px-3 py-2"
            >
              <div className="min-w-0">
                <div className="flex items-center gap-2">
                  <StatusPill tone={row.linked ? "ok" : "neutral"}>
                    {row.linked ? "linked" : "unlinked"}
                  </StatusPill>
                  <span className="font-medium capitalize text-slate-100">
                    {row.channel}
                  </span>
                  <StatusPill tone={row.enabled ? "ok" : "neutral"}>
                    {row.enabled ? "enabled" : "disabled"}
                  </StatusPill>
                </div>
                <p className="mt-0.5 text-xs text-slate-500">
                  {`${row.messageCount} messages this session`}
                </p>
              </div>
              <div className="flex items-center gap-2">
                {!row.linked ? (
                  <>
                    <input
                      type="password"
                      aria-label={`${row.channel} token or webhook URL`}
                      placeholder={
                        row.channel === "discord"
                          ? "Bot token or webhook URL"
                          : "Bot token"
                      }
                      value={tokens[row.channel] ?? ""}
                      onChange={(event) => {
                        setTokens((prev) => ({
                          ...prev,
                          [row.channel]: event.target.value,
                        }));
                      }}
                      className="w-56 rounded-md border border-white/15 bg-void-950 px-2 py-1 text-sm text-slate-100 placeholder:text-slate-600"
                    />
                    <NeonButton
                      variant="primary"
                      disabled={busy === row.channel}
                      onClick={() => {
                        void link(row.channel);
                      }}
                    >
                      {busy === row.channel ? "Linking…" : "Link bot"}
                    </NeonButton>
                  </>
                ) : (
                  <NeonButton
                    variant="ghost"
                    disabled={busy === row.channel}
                    onClick={() => {
                      void unlink(row.channel);
                    }}
                  >
                    {busy === row.channel ? "Unlinking…" : "Unlink"}
                  </NeonButton>
                )}
              </div>
            </li>
          ))}
        </ul>
      )}
      {messages.length > 0 ? (
        <div className="mt-3">
          <h3 className="mb-1 text-xs font-semibold uppercase tracking-wider text-slate-500">
            Recent traffic
          </h3>
          <ul
            aria-label="Recent channel traffic"
            className="flex max-h-48 flex-col gap-1 overflow-y-auto"
          >
            {messages.map((message, index) => (
              <li
                // biome-ignore lint/suspicious/noArrayIndexKey: log has no stable ids
                key={`${message.at}-${index}`}
                className="truncate text-xs text-slate-400"
              >
                <span className="font-mono text-slate-500">{message.at}</span>
                {` [${message.channel}/${message.direction}] ${message.chatId}: ${message.excerpt}`}
              </li>
            ))}
          </ul>
        </div>
      ) : null}
      {error !== null && (channels === null || channels.length > 0) ? (
        <p className="mt-2 text-xs text-amber-300" role="status">
          {error}
        </p>
      ) : null}
      {channels !== null && channels.length > 0 ? (
        <div className="mt-2">
          <NeonButton
            variant="ghost"
            onClick={() => {
              void refresh();
            }}
          >
            Refresh
          </NeonButton>
        </div>
      ) : null}
    </GlassPanel>
  );
}
