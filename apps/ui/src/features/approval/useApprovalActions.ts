import { useCallback, useState } from "react";
import type { ApprovalRequest } from "@talos/contracts";
import { TalosIdSchema, TaskIdSchema } from "@talos/contracts";
import { isBackendUnavailable } from "../../bridge/errors.js";
import {
  batchApproveLowRisk,
  denyApproval,
  executeCompensation,
  getPendingApprovals,
  grantApproval,
  isTauri,
  modifyApproval,
} from "../../bridge/index.js";
import { useApprovalStore } from "../../stores/approvalStore.js";
import { getMockCompensation } from "./mockApprovals.js";

/**
 * Approval actions: Tauri IPC when available, deterministic mock updates in
 * browser preview. All transitions funnel through the approval store so the
 * sheet, queue, status bar, and Void Neon stay in sync.
 */
export function useApprovalActions() {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const upsert = useApprovalStore((s) => s.upsertApproval);
  const remove = useApprovalStore((s) => s.removeApproval);
  const setPending = useApprovalStore((s) => s.setPending);
  const setCompensation = useApprovalStore((s) => s.setCompensation);

  const approve = useCallback(
    async (approval: ApprovalRequest, dontAskAgain: boolean) => {
      setBusy(true);
      setError(null);
      try {
        if (!isTauri()) {
          remove(approval.approvalId);
          return;
        }
        const next = await grantApproval(approval.approvalId, dontAskAgain);
        upsert(next);
        if (next.status !== "pending") remove(next.approvalId);
      } catch (err) {
        if (isBackendUnavailable(err)) {
          remove(approval.approvalId);
          return;
        }
        setError(err instanceof Error ? err.message : "Approve failed");
      } finally {
        setBusy(false);
      }
    },
    [remove, upsert],
  );

  const deny = useCallback(
    async (approval: ApprovalRequest, reason: string) => {
      setBusy(true);
      setError(null);
      try {
        if (!isTauri()) {
          remove(approval.approvalId);
          return;
        }
        const next = await denyApproval(approval.approvalId, reason);
        upsert(next);
        if (next.status !== "pending") remove(next.approvalId);
      } catch (err) {
        if (isBackendUnavailable(err)) {
          remove(approval.approvalId);
          return;
        }
        setError(err instanceof Error ? err.message : "Deny failed");
      } finally {
        setBusy(false);
      }
    },
    [remove, upsert],
  );

  const modify = useCallback(
    async (approval: ApprovalRequest, params: unknown) => {
      setBusy(true);
      setError(null);
      try {
        if (!isTauri()) {
          upsert({
            ...approval,
            payload: params,
            summary: `${approval.summary} (modified by reviewer)`,
            status: "pending",
          });
          return;
        }
        const next = await modifyApproval(approval.approvalId, params);
        upsert(next);
      } catch (err) {
        setError(err instanceof Error ? err.message : "Modify failed");
      } finally {
        setBusy(false);
      }
    },
    [upsert],
  );

  const batchApprove = useCallback(
    async (taskId: string) => {
      setBusy(true);
      setError(null);
      try {
        if (!isTauri()) {
          const state = useApprovalStore.getState();
          for (const a of state.pending) {
            if (
              a.taskId === taskId &&
              a.riskLevel === "low" &&
              a.reversibility === "reversible"
            ) {
              remove(a.approvalId);
            }
          }
          return;
        }
        const approved = await batchApproveLowRisk(taskId);
        for (const a of approved) remove(a.approvalId);
      } catch (err) {
        setError(err instanceof Error ? err.message : "Batch approve failed");
      } finally {
        setBusy(false);
      }
    },
    [remove],
  );

  const refresh = useCallback(async () => {
    if (!isTauri()) {
      return;
    }
    try {
      const list = await getPendingApprovals();
      setPending(list);
    } catch {
      // Preview / offline: keep current queue.
    }
  }, [setPending]);

  const runCompensation = useCallback(
    async (planId: string, taskId: string, failureReason: string) => {
      setBusy(true);
      setError(null);
      try {
        if (!isTauri()) {
          const mock = getMockCompensation();
          const planParsed = TalosIdSchema.safeParse(planId);
          const taskParsed = TaskIdSchema.safeParse(taskId);
          setCompensation({
            ...mock,
            planId: planParsed.success ? planParsed.data : mock.planId,
            taskId: taskParsed.success ? taskParsed.data : mock.taskId,
            failureReason,
            status: "completed",
            stepsCompleted: mock.steps.length,
            steps: mock.steps.map((s) => ({ ...s, status: "done" })),
            finishedAt: new Date().toISOString(),
          });
          return;
        }
        const exec = await executeCompensation(planId, taskId, failureReason, []);
        setCompensation(exec);
      } catch (err) {
        setError(err instanceof Error ? err.message : "Compensation failed");
      } finally {
        setBusy(false);
      }
    },
    [setCompensation],
  );

  return { approve, deny, modify, batchApprove, refresh, runCompensation, busy, error };
}
