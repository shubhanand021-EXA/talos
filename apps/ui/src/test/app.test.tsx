import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { App } from "../app/App.js";

afterEach(() => {
  vi.restoreAllMocks();
});

describe("app shell", () => {
  it("renders brand, nav, and dashboard", () => {
    render(<App />);
    expect(
      screen.getByRole("banner", { name: "Application" }),
    ).toHaveTextContent("Talos");
    expect(
      screen.getByRole("navigation", { name: "Primary" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: "Dashboard" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("contentinfo")).toBeInTheDocument();
  });

  it("navigates between all routes without blank pages", async () => {
    const user = userEvent.setup();
    render(<App />);
    const expectations: Array<[string, string]> = [
      ["Agents", "Orchestrator Prime"],
      ["Tools", "No tools yet"],
      ["Browser", "No browser tools yet"],
      ["Computer", "No computer tools yet"],
      ["Evidence", "Open claims"],
      ["Audit", "Batch 1 green baseline"],
      ["Settings", "Appearance"],
    ];
    for (const [link, region] of expectations) {
      await user.click(screen.getByRole("link", { name: link }));
      expect(await screen.findByText(region)).toBeInTheDocument();
    }
    await user.click(screen.getByRole("link", { name: "Dashboard" }));
    expect(await screen.findByText("Recent activity")).toBeInTheDocument();
  });

  it("emits no console errors on dashboard render", () => {
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    render(<App />);
    expect(spy).not.toHaveBeenCalled();
  });
});
