import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AuditPage } from "./AuditPage.js";

const LIVE = {
  generatedAt: "2026-10-31T15:00:00.000Z",
  root: "/repo",
  configVersion: 1,
  status: "PASS",
  summary: {
    agents: 9,
    passed: 9,
    warned: 0,
    failed: 0,
    findings: 3,
    critical: 0,
    warnings: 0,
    infos: 3,
  },
  agents: [
    {
      agent: "SecurityAuditAgent",
      status: "pass",
      findings: [],
      durationMs: 12,
    },
  ],
  metrics: { "ui.distBytes": 100 },
};

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("AuditPage", () => {
  it("falls back to the committed snapshot when no live report is served", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockRejectedValue(new Error("no public copy")),
    );
    render(<AuditPage />);
    expect(screen.getByText(/Loading audit report/)).toBeInTheDocument();
    expect(await screen.findByText("committed snapshot")).toBeInTheDocument();
    expect(screen.getByText(/Swarm (PASS|WARN|FAIL)/)).toBeInTheDocument();
    expect(
      screen.getByText("docs/reports/audit-latest.json"),
    ).toBeInTheDocument();
  });

  it("renders the live report with agent grid and baseline gates", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: true,
        json: () => Promise.resolve(LIVE),
      }),
    );
    render(<AuditPage />);
    expect(await screen.findByText("live report")).toBeInTheDocument();
    expect(await screen.findByText("SecurityAuditAgent")).toBeInTheDocument();
    expect(screen.getByText("ui.distBytes")).toBeInTheDocument();
    await waitFor(() => {
      expect(
        screen.queryByText("Loading audit report"),
      ).not.toBeInTheDocument();
    });
  });

  it("falls back to the committed snapshot when the live copy is invalid", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: true,
        json: () => Promise.resolve({ nope: true }),
      }),
    );
    render(<AuditPage />);
    expect(await screen.findByText("committed snapshot")).toBeInTheDocument();
    expect(screen.getByText(/Swarm (PASS|WARN|FAIL)/)).toBeInTheDocument();
  });
});
