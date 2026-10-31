import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { SceneErrorBoundary } from "./SceneErrorBoundary.js";
import { SceneSettingsProvider } from "./SceneSettingsProvider.js";
import { useSceneSettings } from "./scene-settings.js";
import { VoidNeonViewport } from "./VoidNeonViewport.js";
import { isWebGLAvailable } from "./webgl.js";

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
});
