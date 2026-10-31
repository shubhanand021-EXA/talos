import { useEffect } from "react";
import { Outlet } from "react-router-dom";
import { useUiStore } from "../stores/uiStore.js";
import { CommandPalette } from "../design-system/CommandPalette.js";
import { ErrorBoundary } from "../design-system/ErrorBoundary.js";
import { BottomStatusBar } from "./chrome/BottomStatusBar.js";
import { LeftRail } from "./chrome/LeftRail.js";
import { RightInspector } from "./chrome/RightInspector.js";
import { TopBar } from "./chrome/TopBar.js";

export function Shell() {
  const paletteOpen = useUiStore((state) => state.commandPaletteOpen);
  const setPaletteOpen = useUiStore((state) => state.setCommandPaletteOpen);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        setPaletteOpen(true);
      }
    };
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("keydown", onKey);
    };
  }, [setPaletteOpen]);

  return (
    <div className="flex h-full flex-col bg-void-950 text-slate-100">
      <TopBar />
      <div className="flex min-h-0 flex-1">
        <LeftRail />
        <main
          className="min-w-0 flex-1 overflow-y-auto p-4 lg:p-6"
          aria-label="Main viewport"
        >
          <ErrorBoundary>
            <Outlet />
          </ErrorBoundary>
        </main>
        <RightInspector />
      </div>
      <BottomStatusBar />
      {paletteOpen ? (
        <CommandPalette
          onClose={() => {
            setPaletteOpen(false);
          }}
        />
      ) : null}
    </div>
  );
}
