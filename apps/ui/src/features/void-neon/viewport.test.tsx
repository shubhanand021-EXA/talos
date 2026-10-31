import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { TalosIdSchema } from "@talos/contracts";
import { SceneErrorBoundary } from "./SceneErrorBoundary.js";
import { SceneHud } from "./SceneHud.js";
import { SceneSettingsProvider } from "./SceneSettingsProvider.js";
import { useSceneSettings } from "./scene-settings.js";
import { ViewportControls } from "./ViewportControls.js";
import { VoidNeonViewport } from "./VoidNeonViewport.js";
import { isWebGLAvailable } from "./webgl.js";
import { KIND_FILTER_GROUPS } from "./topology.js";

describe("void-neon viewport", () => {
  it("reports WebGL unavailable in jsdom (fallback path)", () => {
    expect(isWebGLAvailable()).toBe(false);
  });

  it("renders a polished fallback instead of a blank canvas", () => {
    render(<VoidNeonViewport />);
    expect(screen.getByText("Viewport unavailable")).toBeInTheDocument();
    expect(
      screen.getByText(/WebGL could not be initialized/i),
    ).toBeInTheDocument();
  });

  it("exposes quality switching through settings", async () => {
    const user = userEvent.setup();
    function Probe() {
      const { quality, setQuality } = useSceneSettings();
      return (
        <button
          type="button"
          onClick={() => {
            setQuality("low");
          }}
        >
          {quality}
        </button>
      );
    }
    render(
      <SceneSettingsProvider>
        <Probe />
      </SceneSettingsProvider>,
    );
    expect(screen.getByRole("button", { name: "medium" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "medium" }));
    expect(screen.getByRole("button", { name: "low" })).toBeInTheDocument();
  });

  it("throws outside the provider (fail-fast wiring)", () => {
    function Probe() {
      useSceneSettings();
      return null;
    }
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    expect(() => render(<Probe />)).toThrow();
    spy.mockRestore();
  });

  it("scene error boundary contains render faults", () => {
    function Boom(): never {
      throw new Error("gl context lost");
    }
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    try {
      render(
        <SceneErrorBoundary>
          <Boom />
        </SceneErrorBoundary>,
      );
      expect(screen.getByRole("alert")).toBeInTheDocument();
      expect(screen.getByText("Viewport recovered")).toBeInTheDocument();
    } finally {
      spy.mockRestore();
    }
  });

  it("HUD shows live counts, safety, and ledger state", () => {
    render(
      <SceneSettingsProvider>
        <SceneHud
          activeCount={12}
          selected={null}
          hovered={null}
          fps={60}
          counts={{
            tasksActive: 3,
            waitingPolicy: 1,
            browserEvents: 4,
            computerEvents: 2,
          }}
          safety={{ stopped: false, enabled: false }}
          ledgerHealthy
          evidenceVersion={7}
        />
      </SceneSettingsProvider>,
    );
    expect(screen.getByText("12 nodes live")).toBeInTheDocument();
    expect(screen.getByText("no selection")).toBeInTheDocument();
    expect(screen.getByText("policy gate 1")).toBeInTheDocument();
    expect(screen.getByText("tasks 3")).toBeInTheDocument();
    expect(screen.getByText("browser 4 · computer 2")).toBeInTheDocument();
    expect(screen.getByText("computer safe")).toBeInTheDocument();
    expect(screen.getByText("ledger verified · v7")).toBeInTheDocument();
    expect(screen.getByText("60 fps")).toBeInTheDocument();
  });

  it("HUD raises the global emergency-stop overlay", () => {
    render(
      <SceneSettingsProvider>
        <SceneHud
          activeCount={12}
          selected={null}
          hovered={null}
          fps={0}
          counts={{
            tasksActive: 0,
            waitingPolicy: 0,
            browserEvents: 0,
            computerEvents: 0,
          }}
          safety={{ stopped: true, enabled: false }}
          ledgerHealthy
          evidenceVersion={0}
        />
      </SceneSettingsProvider>,
    );
    expect(
      screen.getByRole("alert", { name: "Emergency stop active" }),
    ).toBeInTheDocument();
    expect(screen.getByText("STOPPED")).toBeInTheDocument();
  });

  it("HUD shows hover tooltips over the selected node", () => {
    render(
      <SceneSettingsProvider>
        <SceneHud
          activeCount={3}
          selected={null}
          hovered={{
            id: TalosIdSchema.parse("task-1"),
            label: "Do work",
            kind: "task",
            status: "active",
            metadata: { queueStatus: "executing" },
          }}
          fps={0}
          counts={{
            tasksActive: 1,
            waitingPolicy: 0,
            browserEvents: 0,
            computerEvents: 0,
          }}
          safety={null}
          ledgerHealthy
          evidenceVersion={0}
        />
      </SceneSettingsProvider>,
    );
    expect(screen.getByRole("status")).toHaveTextContent(
      "Do work · task · executing",
    );
  });

  it("controls toggle kind filters and focus mode", async () => {
    const user = userEvent.setup();
    const toggled: string[] = [];
    let focus = false;
    const { rerender } = render(
      <ViewportControls
        visibleKinds={new Set(KIND_FILTER_GROUPS.map(({ kind }) => kind))}
        onToggleKind={(kind) => {
          toggled.push(kind);
        }}
        focusMode={focus}
        onToggleFocus={() => {
          focus = !focus;
        }}
        hasSelection
        replayEvents={[]}
        replayIndex={null}
        onReplayChange={() => {}}
      />,
    );
    for (const { label } of KIND_FILTER_GROUPS) {
      expect(screen.getByRole("button", { name: label })).toHaveAttribute(
        "aria-pressed",
        "true",
      );
    }
    await user.click(screen.getByRole("button", { name: "Browser" }));
    expect(toggled).toEqual(["browser"]);
    await user.click(screen.getByRole("button", { name: "focus" }));
    expect(focus).toBe(true);
    rerender(
      <ViewportControls
        visibleKinds={new Set()}
        onToggleKind={() => {}}
        focusMode={false}
        onToggleFocus={() => {}}
        hasSelection={false}
        replayEvents={[]}
        replayIndex={null}
        onReplayChange={() => {}}
      />,
    );
    expect(screen.getByRole("button", { name: "focus" })).toBeDisabled();
  });

  it("controls scrub recent events on the replay slider", async () => {
    const user = userEvent.setup();
    const seen: Array<number | null> = [];
    render(
      <ViewportControls
        visibleKinds={new Set()}
        onToggleKind={() => {}}
        focusMode={false}
        onToggleFocus={() => {}}
        hasSelection={false}
        replayEvents={[
          { id: "e1", label: "runtime: task-1 started", taskId: "task-1" },
          { id: "e2", label: "policy: task-2 held", taskId: "task-2" },
        ]}
        replayIndex={null}
        onReplayChange={(index) => {
          seen.push(index);
        }}
      />,
    );
    expect(screen.getByText("live edge")).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("replay"), {
      target: { value: "0" },
    });
    expect(seen).toEqual([0]);
    await user.click(screen.getByRole("button", { name: "live" }));
    expect(seen).toEqual([0, null]);
  });
});
