import { beforeEach, describe, expect, it } from "vitest";
import { useAppStore } from "../stores/appStore.js";
import {
  getMockAgents,
  getMockTasks,
  useMockStore,
} from "../stores/mockStore.js";
import { useUiStore } from "../stores/uiStore.js";

beforeEach(() => {
  useUiStore.setState({
    selectedEntity: null,
    themePreference: "dark",
    commandPaletteOpen: false,
  });
  useAppStore.setState({ connectionState: "connected" });
  useMockStore.setState({ ready: false });
});

describe("stores", () => {
  it("updates connection state", () => {
    useAppStore.getState().setConnectionState("connecting");
    expect(useAppStore.getState().connectionState).toBe("connecting");
  });

  it("updates theme and selection", () => {
    useUiStore.getState().setThemePreference("light");
    expect(useUiStore.getState().themePreference).toBe("light");
    useUiStore.getState().selectEntity({
      kind: "agent",
      id: "agent-1",
      label: "Coder",
      detail: "coder · running",
    });
    expect(useUiStore.getState().selectedEntity?.id).toBe("agent-1");
  });

  it("hydrates mock store and serves deterministic fixtures", () => {
    useMockStore.getState().hydrate();
    expect(useMockStore.getState().ready).toBe(true);
    expect(getMockAgents()).toHaveLength(3);
    expect(getMockTasks()).toHaveLength(3);
  });
});
