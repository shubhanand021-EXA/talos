import { describe, expect, it, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import type { PlanPreviewView } from "@talos/contracts";
import { PlanViewer } from "./PlanViewer.js";

function plan(overrides: Partial<PlanPreviewView> = {}): PlanPreviewView {
  return {
    parentTaskId: "parent-1" as PlanPreviewView["parentTaskId"],
    method: "llm",
    steps: [
      {
        stepId: "a",
        role: "researcher",
        goal: "gather context",
        dependsOn: [],
        toolsNeeded: ["fs.read"],
        requiresApproval: false,
        estimatedTokens: 100,
      },
      {
        stepId: "b",
        role: "coder",
        goal: "apply the fix",
        dependsOn: ["a"],
        toolsNeeded: ["fs.read"],
        requiresApproval: true,
        estimatedTokens: 200,
      },
    ],
    levels: [["a"], ["b"]],
    requiresApproval: true,
    fallbackReason: null,
    valid: true,
    ...overrides,
  };
}

describe("PlanViewer", () => {
  it("renders the DAG with roles, dependencies, and risk", () => {
    render(<PlanViewer plan={plan()} />);
    expect(screen.getByTestId("plan-step-a")).toBeTruthy();
    expect(screen.getByTestId("plan-step-b")).toBeTruthy();
    expect(screen.getByText("LLM")).toBeTruthy();
    expect(screen.getByText("valid")).toBeTruthy();
    expect(screen.getByText("requires approval")).toBeTruthy();
    expect(screen.getByText("depends on: a")).toBeTruthy();
    expect(screen.getByText("high-risk")).toBeTruthy();
  });

  it("displays validation status and fallback reason", () => {
    render(
      <PlanViewer
        plan={plan({
          method: "keyword",
          valid: true,
          requiresApproval: false,
          fallbackReason: "no provider key configured",
        })}
      />,
    );
    expect(screen.getByText("keyword fallback")).toBeTruthy();
    expect(screen.getByText(/no provider key/)).toBeTruthy();
  });

  it("approve and reject buttons work", () => {
    const onApprove = vi.fn();
    const onReject = vi.fn();
    render(
      <PlanViewer plan={plan()} onApprove={onApprove} onReject={onReject} />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Approve plan" }));
    fireEvent.click(screen.getByRole("button", { name: "Reject plan" }));
    expect(onApprove).toHaveBeenCalledTimes(1);
    expect(onReject).toHaveBeenCalledTimes(1);
  });
});
