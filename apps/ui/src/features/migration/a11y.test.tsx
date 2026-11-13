import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { MigrationPanel } from "./MigrationPanel.js";
import { useMigrationStore } from "../../stores/migrationStore.js";

const { mockScan, mockPreview } = vi.hoisted(() => ({
  mockScan: vi.fn(),
  mockPreview: vi.fn(),
}));

vi.mock("../../bridge/index.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../bridge/index.js")>();
  return {
    ...actual,
    isTauri: () => false,
    migrationScan: mockScan,
    migrationPreview: mockPreview,
    migrationApply: vi.fn(),
  };
});

const SCAN = {
  importId: "migration-a11y",
  workspacePath: "/tmp/ws",
  findings: [
    {
      sourcePath: "/tmp/ws/SKILL.md",
      kind: "skill",
      category: "skills",
      flagged: false,
      riskLabels: [],
      secretStripped: false,
      note: "clean",
    },
  ],
  scannedAt: "2026-11-14T00:00:00.000Z",
};

const PREVIEW = {
  importId: "migration-a11y",
  items: [
    {
      category: "skills",
      action: "quarantine-skill",
      target: "skill:/tmp/ws/SKILL.md",
      defaultState: "quarantined",
      riskLabels: [],
      secretStripped: false,
    },
  ],
  previewedAt: "2026-11-14T00:00:00.000Z",
};

beforeEach(() => {
  useMigrationStore.setState({
    scan: null,
    preview: null,
    applyResult: null,
    busy: false,
    error: null,
    announcement: null,
    subscribed: true,
  });
  mockScan.mockReset().mockResolvedValue(SCAN);
  mockPreview.mockReset().mockResolvedValue(PREVIEW);
});

/** Relative luminance per WCAG 2.1 (sRGB). */
function luminance(hex: string): number {
  const rgb = [1, 3, 5].map((i) => {
    const c = parseInt(hex.slice(i, i + 2), 16) / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * rgb[0]! + 0.7152 * rgb[1]! + 0.0722 * rgb[2]!;
}

function contrastRatio(foreground: string, background: string): number {
  const [lighter, darker] = [luminance(foreground), luminance(background)].sort(
    (a, b) => b - a,
  );
  return (lighter! + 0.05) / (darker! + 0.05);
}

describe("MigrationPanel accessibility (WCAG 2.1 AA)", () => {
  it("gives every control an accessible name and label", async () => {
    const user = userEvent.setup();
    const { container } = render(<MigrationPanel />);

    await user.type(screen.getByLabelText(/workspace path to scan/i), "/tmp");
    await user.click(screen.getByRole("button", { name: "Scan" }));
    await screen.findByText("/tmp/ws/SKILL.md");
    await user.click(screen.getByRole("button", { name: "Preview import" }));
    await screen.findByText(/what import would create/i);

    for (const button of screen.getAllByRole("button")) {
      // Real accessible-name computation (jest-dom ARIA algorithm).
      expect(button).toHaveAccessibleName();
    }
    for (const input of container.querySelectorAll("input")) {
      const labelled =
        input.getAttribute("aria-label") !== null ||
        input.getAttribute("aria-labelledby") !== null ||
        (input.getAttribute("id") !== null &&
          container.querySelector(
            `label[for="${input.getAttribute("id")}"]`,
          ) !== null);
      expect(labelled).toBe(true);
    }
    for (const element of container.querySelectorAll("[tabindex]")) {
      expect(Number(element.getAttribute("tabindex"))).toBeLessThanOrEqual(0);
    }
  });

  it("announces scan results in a live region", async () => {
    const user = userEvent.setup();
    render(<MigrationPanel />);
    await user.type(screen.getByLabelText(/workspace path to scan/i), "/tmp");
    await user.click(screen.getByRole("button", { name: "Scan" }));
    const status = await screen.findByRole("status");
    expect(status.getAttribute("aria-live")).toBe("polite");
    expect(status.textContent).toMatch(/scan complete/i);
  });

  it("moves focus into the confirm dialog and traps Escape", async () => {
    const user = userEvent.setup();
    render(<MigrationPanel />);
    await user.type(screen.getByLabelText(/workspace path to scan/i), "/tmp");
    await user.click(screen.getByRole("button", { name: "Scan" }));
    await screen.findByText("/tmp/ws/SKILL.md");
    await user.click(screen.getByRole("button", { name: "Preview import" }));
    await screen.findByText(/what import would create/i);
    await user.click(
      screen.getByLabelText(/approve skills \(SKILL\.MD\) for import/i),
    );
    await user.click(
      screen.getByRole("button", { name: "Import approved categories" }),
    );
    const dialog = await screen.findByRole("dialog");
    expect(dialog.getAttribute("aria-modal")).toBe("true");
    const confirm = within(dialog).getByRole("button", {
      name: "Confirm import",
    });
    expect(document.activeElement).toBe(confirm);
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("meets 4.5:1 contrast for key panel text pairs", () => {
    // Void Neon palette pairs used by the panel (light text on near-black).
    const pairs: Array<[string, string, string]> = [
      ["#e2e8f0", "#05060a", "slate-200 body on void"],
      ["#cbd5e1", "#05060a", "slate-300 notices on void"],
      ["#a5f3fc", "#05060a", "cyan-200 links on void"],
      ["#fda4af", "#05060a", "rose-300 errors on void"],
    ];
    for (const [foreground, background, label] of pairs) {
      expect(
        contrastRatio(foreground, background),
        label,
      ).toBeGreaterThanOrEqual(4.5);
    }
  });
});
