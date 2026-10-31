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
  deleteProviderKey,
  getUsageSummary,
  listModels,
  listProviders,
  storeProviderKey,
  subscribeTalosEvent,
  testProviderConnection,
  LlmRequestCompletedPayloadSchema,
  type ModelDescriptor,
  type ProviderStatus,
  type UsageSummary,
} from "../../bridge/index.js";

function toneFor(connected: boolean | null): "ok" | "warn" | "neutral" {
  if (connected === true) {
    return "ok";
  }
  if (connected === false) {
    return "warn";
  }
  return "neutral";
}

function labelFor(connected: boolean | null): string {
  if (connected === true) {
    return "reachable";
  }
  if (connected === false) {
    return "unreachable";
  }
  return "unknown";
}

export function ProviderSettingsPanel() {
  const [providers, setProviders] = useState<ProviderStatus[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [models, setModels] = useState<ModelDescriptor[]>([]);
  const [usage, setUsage] = useState<UsageSummary | null>(null);
  const [keyInput, setKeyInput] = useState("");
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [loaded, setLoaded] = useState(false);

  const refresh = useCallback(async () => {
    const [nextProviders, nextUsage] = await Promise.all([
      listProviders(),
      getUsageSummary(),
    ]);
    setProviders(nextProviders);
    setUsage(nextUsage);
    setLoaded(true);
  }, []);

  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      try {
        const [nextProviders, nextUsage] = await Promise.all([
          listProviders(),
          getUsageSummary(),
        ]);
        if (!cancelled) {
          setProviders(nextProviders);
          setUsage(nextUsage);
          setNotice(null);
        }
      } catch (error: unknown) {
        if (!cancelled) {
          setNotice(error instanceof Error ? error.message : String(error));
        }
      } finally {
        if (!cancelled) {
          setLoaded(true);
        }
      }
    };
    void load();
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    const watch = async () => {
      try {
        const stop = await subscribeTalosEvent(
          "llmRequestCompleted",
          () => {
            void refresh().catch(() => undefined);
          },
          LlmRequestCompletedPayloadSchema,
        );
        if (cancelled) {
          stop();
        } else {
          unlisten = stop;
        }
      } catch {
        // Browser preview has no Tauri events; manual refresh still works.
      }
    };
    void watch();
    return () => {
      cancelled = true;
      unlisten?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const selectProvider = useCallback(async (providerId: string) => {
    setSelected(providerId);
    setModels(await listModels(providerId));
  }, []);

  async function run<T>(action: () => Promise<T>): Promise<T | undefined> {
    setBusy(true);
    setNotice(null);
    try {
      return await action();
    } catch (error: unknown) {
      setNotice(error instanceof Error ? error.message : String(error));
      return undefined;
    } finally {
      setBusy(false);
    }
  }

  if (!loaded) {
    return <Spinner label="Loading providers" />;
  }

  return (
    <div className="flex flex-col gap-4">
      {providers.map((provider) => (
        <GlassPanel key={provider.providerId} label={provider.providerId}>
          <div className="flex flex-wrap items-center gap-3">
            <span className="text-sm text-slate-200">
              {provider.providerId}
            </span>
            <StatusPill tone={provider.configured ? "ok" : "neutral"}>
              {provider.configured ? "key stored" : "no key"}
            </StatusPill>
            <StatusPill tone={toneFor(provider.reachable)}>
              {labelFor(provider.reachable)}
            </StatusPill>
            {provider.latencyMs !== null && (
              <span className="text-xs text-slate-400">
                {provider.latencyMs} ms
              </span>
            )}
            <NeonButton
              variant="ghost"
              disabled={busy}
              onClick={() => {
                void run(async () => selectProvider(provider.providerId));
              }}
            >
              Models
            </NeonButton>
            <NeonButton
              variant="ghost"
              disabled={busy}
              onClick={() => {
                void run(async () => {
                  const result = await testProviderConnection(
                    provider.providerId,
                  );
                  setNotice(
                    `${result.providerId}: ${result.ok ? "reachable" : "unreachable"} — ${result.message}`,
                  );
                  await refresh();
                });
              }}
            >
              Test connection
            </NeonButton>
            <NeonButton
              variant="ghost"
              disabled={busy || !provider.configured}
              onClick={() => {
                void run(async () => {
                  await deleteProviderKey(provider.providerId);
                  setKeyInput("");
                  await refresh();
                });
              }}
            >
              Remove key
            </NeonButton>
          </div>
          <div className="mt-3 flex flex-wrap items-center gap-2">
            <input
              type="password"
              aria-label={`API key for ${provider.providerId}`}
              autoComplete="off"
              spellCheck={false}
              className="rounded border border-slate-700 bg-slate-900 px-2 py-1 text-sm text-slate-100"
              placeholder="Paste API key (never displayed again)"
              value={selected === provider.providerId ? keyInput : ""}
              onChange={(event) => {
                setSelected(provider.providerId);
                setKeyInput(event.target.value);
              }}
            />
            <NeonButton
              variant="ghost"
              disabled={busy || keyInput.trim() === ""}
              onClick={() => {
                const value = keyInput;
                setKeyInput("");
                void run(async () => {
                  await storeProviderKey(provider.providerId, value);
                  await refresh();
                });
              }}
            >
              Add key
            </NeonButton>
          </div>
          {selected === provider.providerId && models.length > 0 && (
            <ul className="mt-3 flex flex-col gap-1">
              {models.map((model) => (
                <li key={model.modelId} className="text-xs text-slate-400">
                  {model.modelId} · {model.contextWindow} ctx
                  {model.supportsTools ? " · tools" : ""}
                  {model.supportsVision ? " · vision" : ""}
                  {model.costPer1kInput > 0
                    ? ` · $${model.costPer1kInput}/1k in`
                    : " · local"}
                </li>
              ))}
            </ul>
          )}
        </GlassPanel>
      ))}
      {usage !== null && (
        <GlassPanel label="Session usage">
          <KeyValueList
            entries={[
              { key: "totalTokens", value: String(usage.totalTokens) },
              {
                key: "estimatedCostUsd",
                value: usage.estimatedCostUsd.toFixed(6),
              },
              { key: "calls", value: String(usage.callCount) },
            ]}
          />
        </GlassPanel>
      )}
      {notice !== null && (
        <p className="text-xs text-slate-400" role="status">
          {notice}
        </p>
      )}
    </div>
  );
}

export function ProvidersSection() {
  return (
    <GlassPanel label="Model providers">
      <SectionHeader
        title="Model providers"
        subtitle="BYOK keys live in the OS keychain — values are never displayed"
      />
      <ProviderSettingsPanel />
    </GlassPanel>
  );
}
