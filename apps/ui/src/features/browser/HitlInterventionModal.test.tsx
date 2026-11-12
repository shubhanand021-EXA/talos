import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { HitlInterventionModal } from "./HitlInterventionModal.js";

const intervention = {
  interventionId: "hitl-1",
  taskId: "task-1",
  kind: "captcha_detected" as const,
  url: "https://example.com/protected",
  reason: "cloudflare-challenge",
  checkedAt: "2026-11-08T00:00:00.000Z",
};

describe("HitlInterventionModal", () => {
  it("renders the intervention and resumes on click", async () => {
    const user = userEvent.setup();
    const onResume = vi.fn(() => Promise.resolve());
    const onDismiss = vi.fn(() => Promise.resolve());
    const onClose = vi.fn();
    render(
      <HitlInterventionModal
        intervention={intervention}
        onResume={onResume}
        onDismiss={onDismiss}
        onClose={onClose}
      />,
    );
    expect(
      screen.getByText("Talos requires manual browser intervention"),
    ).toBeTruthy();
    expect(screen.getByText("cloudflare-challenge")).toBeTruthy();
    await user.click(screen.getByText("Resume"));
    expect(onResume).toHaveBeenCalledWith("hitl-1");
    expect(onDismiss).not.toHaveBeenCalled();
    expect(onClose).toHaveBeenCalled();
  });

  it("dismisses without resuming", async () => {
    const user = userEvent.setup();
    const onResume = vi.fn(() => Promise.resolve());
    const onDismiss = vi.fn(() => Promise.resolve());
    const onClose = vi.fn();
    render(
      <HitlInterventionModal
        intervention={{ ...intervention, kind: "auth_required", url: null }}
        onResume={onResume}
        onDismiss={onDismiss}
        onClose={onClose}
      />,
    );
    expect(screen.getByText("login wall")).toBeTruthy();
    await user.click(screen.getByText("Dismiss"));
    expect(onDismiss).toHaveBeenCalledWith("hitl-1");
    expect(onResume).not.toHaveBeenCalled();
  });

  it("surfaces resolution failures without closing", async () => {
    const user = userEvent.setup();
    const onResume = vi.fn(() =>
      Promise.reject(new Error("gateway unreachable")),
    );
    const onClose = vi.fn();
    render(
      <HitlInterventionModal
        intervention={intervention}
        onResume={onResume}
        onDismiss={vi.fn(() => Promise.resolve())}
        onClose={onClose}
      />,
    );
    await user.click(screen.getByText("Resume"));
    expect(await screen.findByText("gateway unreachable")).toBeTruthy();
    expect(onClose).not.toHaveBeenCalled();
  });
});
