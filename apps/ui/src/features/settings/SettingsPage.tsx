import { Suspense, lazy } from "react";
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

const LazyProvidersSection = lazy(() =>
  import("./ProviderSettingsPanel.js").then((module) => ({
    default: module.ProvidersSection,
  })),
);

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
    </div>
  );
}
