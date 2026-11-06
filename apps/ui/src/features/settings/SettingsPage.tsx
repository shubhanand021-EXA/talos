import { Suspense, lazy, useCallback, useState } from "react";
import { useAppStore } from "../../stores/appStore.js";
import { useUiStore } from "../../stores/uiStore.js";
import {
  GlassPanel,
  KeyValueList,
  NeonButton,
  SectionHeader,
  Spinner,
  StatusPill,
} from "../../design-system/index.js";
import { isTauri, setOnboardingState } from "../../bridge/index.js";

const LazyProvidersSection = lazy(() =>
  import("./ProviderSettingsPanel.js").then((module) => ({
    default: module.ProvidersSection,
  })),
);

const LazyStorageSection = lazy(() =>
  import("./StoragePanel.js").then((module) => ({
    default: module.StorageSection,
  })),
);

const LazyUpdatesSection = lazy(() =>
  import("./UpdatesPanel.js").then((module) => ({
    default: module.UpdatesSection,
  })),
);

const LazyHeartbeatsSection = lazy(() =>
  import("../scheduler/index.js").then((module) => ({
    default: module.HeartbeatsPanel,
  })),
);

function OnboardingReplaySection() {
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);

  const replay = useCallback(async () => {
    setBusy(true);
    setNotice(null);
    try {
      if (isTauri()) {
        await setOnboardingState(false);
      } else {
        window.localStorage.setItem("talos.onboarding.completed", "false");
      }
    } catch {
      try {
        window.localStorage.setItem("talos.onboarding.completed", "false");
      } catch {
        // Ignore unwritable storage.
      }
    } finally {
      setBusy(false);
    }
    setNotice("Onboarding reset — reloading into the wizard…");
    window.location.reload();
  }, []);

  return (
    <GlassPanel label="Onboarding">
      <SectionHeader
        title="Replay onboarding"
        subtitle="Reset the first-run wizard and walk it again"
      />
      <div className="flex items-center gap-2">
        <NeonButton
          variant="ghost"
          disabled={busy}
          onClick={() => {
            void replay();
          }}
        >
          {busy ? "Resetting…" : "Replay onboarding"}
        </NeonButton>
        {notice !== null ? (
          <span className="text-sm text-slate-300" role="status">
            {notice}
          </span>
        ) : null}
      </div>
    </GlassPanel>
  );
}

export function SettingsPage() {
  const { appInfo, connectionState, setConnectionState } = useAppStore();
  const { themePreference, setThemePreference } = useUiStore();

  return (
    <div className="flex flex-col gap-4">
      <SectionHeader title="Settings" subtitle="Preferences and connection" />
      <div className="grid grid-cols-1 gap-4 2xl:grid-cols-2">
        <GlassPanel label="Application">
          <SectionHeader title="Application" />
          <KeyValueList
            entries={[
              { key: "name", value: appInfo.name },
              { key: "version", value: appInfo.version },
              { key: "environment", value: appInfo.environment },
            ]}
          />
        </GlassPanel>
        <GlassPanel label="Preferences">
          <SectionHeader title="Appearance" subtitle="Void Neon theme family" />
          <div className="flex items-center gap-2">
            <NeonButton
              variant={themePreference === "dark" ? "primary" : "ghost"}
              onClick={() => {
                setThemePreference("dark");
              }}
            >
              Void dark
            </NeonButton>
            <NeonButton
              variant={themePreference === "light" ? "primary" : "ghost"}
              onClick={() => {
                setThemePreference("light");
              }}
            >
              Mist light
            </NeonButton>
          </div>
          <div className="mt-4 flex items-center gap-3">
            <span className="text-sm text-slate-400">Broker link</span>
            <StatusPill tone={connectionState === "connected" ? "ok" : "warn"}>
              {connectionState}
            </StatusPill>
            <NeonButton
              variant="ghost"
              onClick={() => {
                setConnectionState(
                  connectionState === "connected" ? "connecting" : "connected",
                );
              }}
            >
              {connectionState === "connected"
                ? "Simulate dropout"
                : "Reconnect"}
            </NeonButton>
          </div>
        </GlassPanel>
      </div>
      <Suspense fallback={<Spinner label="Loading provider settings" />}>
        <LazyProvidersSection />
      </Suspense>
      <Suspense fallback={<Spinner label="Loading storage settings" />}>
        <LazyStorageSection />
      </Suspense>
      <Suspense fallback={<Spinner label="Loading update settings" />}>
        <LazyUpdatesSection />
      </Suspense>
      <Suspense fallback={<Spinner label="Loading heartbeat schedules" />}>
        <LazyHeartbeatsSection />
      </Suspense>
      <OnboardingReplaySection />
    </div>
  );
}
