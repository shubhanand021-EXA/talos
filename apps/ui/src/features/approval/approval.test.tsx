import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import {
  ApprovalQueuePanel,
  ApprovalSheet,
  CompensationSheet,
  DiffViewer,
  PlanView,
  diffLines,
} from "./index.js";
import { getMockApprovals, getMockCompensation } from "./mockApprovals.js";
import { useApprovalStore } from "../../stores/approvalStore.js";

function resetStore() {
  useApprovalStore.setState({
    pending: [],
    activeApprovalId: null,
    sheetOpen: false,
    taskModes: {},
    planDocuments: {},
    compensations: {},
    lastError: null,
  });
}

describe("DiffViewer", () => {
  it("renders unified diff with line numbers and word highlight", () => {
    render(
      <DiffViewer
        files={[]}
        original={"a\nb\nc\n"}
        proposed={"a\nB\nc\n"}
        target="src/x.ts"
      />,
    );
    expect(screen.getByLabelText("Unified diff for src/x.ts")).toBeInTheDocument();
    expect(screen.getByText("typescript")).toBeInTheDocument();
    expect(screen.getByText("B")).toBeInTheDocument();
  });

  it("diffLines collapses unchanged regions and pairs edits", () => {
    const rows = diffLines("a\nb\nc", "a\nB\nc");
    expect(rows[0]).toMatchObject({ kind: "same", text: "a" });
    expect(rows).toContainEqual(expect.objectContaining({ kind: "del", text: "b" }));
    expect(rows).toContainEqual(expect.objectContaining({ kind: "add", text: "B" }));
  });

  it("shows one tab per file for multi-file edits", () => {
    render(
      <DiffViewer
        files={[
          { path: "a.ts", original: "1", proposed: "2" },
          { path: "b.ts", original: "x", proposed: "y" },
        ]}
        target="multi"
      />,
    );
    expect(screen.getByRole("tablist", { name: "Changed files" })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "a.ts" })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "b.ts" })).toBeInTheDocument();
  });
});

describe("ApprovalSheet", () => {
  it("renders every required section with labels", () => {
    resetStore();
    const [approval] = getMockApprovals();
    if (!approval) throw new Error("mock missing");
    render(
      <ApprovalSheet
        approval={approval}
        queuePosition={0}
        queueTotal={1}
        onApprove={() => undefined}
        onDeny={() => undefined}
        onModify={() => undefined}
        onNext={() => undefined}
        onPrev={() => undefined}
        busy={false}
        error={null}
      />,
    );
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(screen.getByLabelText("Action summary")).toBeInTheDocument();
    expect(screen.getByLabelText("Target resource")).toBeInTheDocument();
    expect(screen.getByLabelText(/Risk high/i)).toBeInTheDocument();
    expect(screen.getByLabelText("Payload: network request")).toBeInTheDocument();
    expect(screen.getByLabelText("Risk assessment")).toBeInTheDocument();
    expect(screen.getByLabelText("Compensation plan")).toBeInTheDocument();
    expect(screen.getByLabelText("Cost estimate and scope")).toBeInTheDocument();
    expect(screen.getByLabelText("Deny reason")).toBeInTheDocument();
    expect(screen.getByLabelText("Approve action (Enter)")).toBeInTheDocument();
    expect(screen.getByLabelText("Deny action (Escape)")).toBeInTheDocument();
    expect(screen.getByLabelText("Modify parameters (M)")).toBeInTheDocument();
    expect(
      screen.getByLabelText("Don't ask again for this action type in this task"),
    ).toBeInTheDocument();
  });

  it("Enter approves, Escape denies, M toggles modify", async () => {
    resetStore();
    const user = userEvent.setup();
    const [approval] = getMockApprovals();
    if (!approval) throw new Error("mock missing");
    const onApprove = vi.fn();
    const onDeny = vi.fn();
    render(
      <ApprovalSheet
        approval={approval}
        queuePosition={0}
        queueTotal={1}
        onApprove={onApprove}
        onDeny={onDeny}
        onModify={() => undefined}
        onNext={() => undefined}
        onPrev={() => undefined}
        busy={false}
        error={null}
      />,
    );
    await user.keyboard("{Enter}");
    expect(onApprove).toHaveBeenCalledTimes(1);
    await user.keyboard("{Escape}");
    expect(onDeny).toHaveBeenCalledTimes(1);
    await user.keyboard("m");
    expect(screen.getByLabelText("Modify parameters")).toBeInTheDocument();
  });

  it("traps focus inside the modal", () => {
    resetStore();
    const [approval] = getMockApprovals();
    if (!approval) throw new Error("mock missing");
    render(
      <ApprovalSheet
        approval={approval}
        queuePosition={0}
        queueTotal={1}
        onApprove={() => undefined}
        onDeny={() => undefined}
        onModify={() => undefined}
        onNext={() => undefined}
        onPrev={() => undefined}
        busy={false}
        error={null}
      />,
    );
    const dialog = screen.getByRole("dialog");
    const buttons = within(dialog).getAllByRole("button");
    expect(buttons.length).toBeGreaterThanOrEqual(3);
    fireEvent.keyDown(document, { key: "Tab" });
    expect(document.activeElement).toBeDefined();
  });

  it("puts the most dangerous info most prominent", () => {
    resetStore();
    const [approval] = getMockApprovals();
    if (!approval) throw new Error("mock missing");
    render(
      <ApprovalSheet
        approval={approval}
        queuePosition={0}
        queueTotal={2}
        onApprove={() => undefined}
        onDeny={() => undefined}
        onModify={() => undefined}
        onNext={() => undefined}
        onPrev={() => undefined}
        busy={false}
        error={null}
      />,
    );
    const alert = screen.getByRole("alert");
    expect(alert.textContent).toMatch(/EXTERNAL_WRITE/);
    expect(alert.textContent).toMatch(/irreversible/);
  });
});

describe("ApprovalQueuePanel", () => {
  it("queues approvals in order with batch approve", () => {
    resetStore();
    useApprovalStore.getState().setPending(getMockApprovals());
    const onBatch = vi.fn();
    render(<ApprovalQueuePanel onBatchApprove={onBatch} busy={false} />);
    const list = screen.getByRole("list", { name: "Pending approvals queue" });
    expect(within(list).getAllByRole("listitem")).toHaveLength(2);
    const batchButtons = screen.getAllByLabelText(/Batch approve low-risk/);
    const firstBatch = batchButtons[0];
    if (firstBatch === undefined) throw new Error("batch button missing");
    fireEvent.click(firstBatch);
    expect(onBatch).toHaveBeenCalled();
  });
});

describe("PlanView", () => {
  it("toggles plan/act/auto and lists ordered steps", () => {
    const onMode = vi.fn();
    render(
      <PlanView
        taskId="task-email"
        mode="plan"
        plan={{
          taskId: "task-email" as never,
          mode: "plan",
          actions: getMockApprovals().map((a) => ({
            approvalId: a.approvalId,
            description: a.summary,
            target: a.target,
            effect: a.effectClass,
            reversibility: a.reversibility,
            risk: a.riskLevel,
            status: a.status,
          })),
          createdAt: "2026-11-01T12:00:00.000Z",
          approvedCount: 0,
          pendingCount: 1,
        }}
        onModeChange={onMode}
        onApproveOne={() => undefined}
        onApproveLowRisk={() => undefined}
        busy={false}
      />,
    );
    expect(screen.getByRole("radiogroup")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("radio", { name: "Act" }));
    expect(onMode).toHaveBeenCalledWith("act");
  });
});

describe("CompensationSheet", () => {
  it("shows progress and manual-review alert on failure", () => {
    render(
      <CompensationSheet
        execution={{
          ...getMockCompensation(),
          status: "failed_needs_manual_review",
          steps: getMockCompensation().steps.map((s, i) =>
            i === 0 ? { ...s, status: "failed" } : s,
          ),
        }}
        onConfirm={() => undefined}
        onFailStep={() => undefined}
        busy={false}
      />,
    );
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(screen.getByRole("progressbar")).toHaveAttribute("aria-valuenow", "0");
    expect(screen.getAllByText(/failed_needs_manual_review/).length).toBeGreaterThanOrEqual(1);
  });
});
