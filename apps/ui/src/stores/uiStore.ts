import { create } from "zustand";
import type { SelectedEntity } from "../types/index.js";

export type ThemePreference = "dark" | "light";

interface UiState {
  commandPaletteOpen: boolean;
  selectedEntity: SelectedEntity | null;
  themePreference: ThemePreference;
  /** Last measured viewport FPS (0 = not measured yet). */
  viewportFps: number;
  setCommandPaletteOpen: (open: boolean) => void;
  selectEntity: (entity: SelectedEntity | null) => void;
  setThemePreference: (theme: ThemePreference) => void;
  setViewportFps: (fps: number) => void;
}

export const useUiStore = create<UiState>()((set) => ({
  commandPaletteOpen: false,
  selectedEntity: null,
  themePreference: "dark",
  viewportFps: 0,
  setCommandPaletteOpen: (commandPaletteOpen) => {
    set({ commandPaletteOpen });
  },
  selectEntity: (selectedEntity) => {
    set({ selectedEntity });
  },
  setThemePreference: (themePreference) => {
    set({ themePreference });
  },
  setViewportFps: (viewportFps) => {
    set({ viewportFps });
  },
}));
