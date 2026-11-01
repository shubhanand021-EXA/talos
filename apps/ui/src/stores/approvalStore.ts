import { create } from "zustand";
import type {
  ApprovalRequest,
  CompensationExecution,
  PlanDocument,
  TaskMode,
} from "@talos/contracts";

interface ApprovalState {
  pending: ApprovalRequest[];
  activeApprovalId: string | null;
  sheetOpen: boolean;
  taskModes: Record<string, TaskMode>;
  planDocuments: Record<string, PlanDocument>;
  compensations: Record<string, CompensationExecution>;
  lastError: string | null;
  setPending: (pending: ApprovalRequest[]) => void;
  upsertApproval: (approval: ApprovalRequest) => void;
  removeApproval: (approvalId: string) => void;
  openSheet: (approvalId?: string | null) => void;
  closeSheet: () => void;
  setActive: (approvalId: string | null) => void;
  setTaskMode: (taskId: string, mode: TaskMode) => void;
  setPlanDocument: (taskId: string, doc: PlanDocument) => void;
  setCompensation: (execution: CompensationExecution) => void;
  setError: (error: string | null) => void;
}

export const useApprovalStore = create<ApprovalState>()((set) => ({
  pending: [],
  activeApprovalId: null,
  sheetOpen: false,
  taskModes: {},
  planDocuments: {},
  compensations: {},
  lastError: null,
  setPending: (pending) => {
    set((state) => {
      const activeStillPending = pending.some(
        (a) => a.approvalId === state.activeApprovalId,
      );
      return {
        pending,
        sheetOpen:
          pending.length > 0 &&
          (state.sheetOpen || !activeStillPending
            ? true
            : state.sheetOpen),
        activeApprovalId:
          state.activeApprovalId !== null && activeStillPending
            ? state.activeApprovalId
            : (pending[0]?.approvalId ?? null),
      };
    });
  },
  upsertApproval: (approval) => {
    set((state) => {
      const existing = state.pending.findIndex(
        (a) => a.approvalId === approval.approvalId,
      );
      let pending: ApprovalRequest[];
      if (existing === -1) {
        pending = approval.status === "pending" ? [...state.pending, approval] : state.pending;
      } else if (approval.status === "pending") {
        pending = [...state.pending];
        pending[existing] = approval;
      } else {
        pending = state.pending.filter(
          (a) => a.approvalId !== approval.approvalId,
        );
      }
      const activeApprovalId =
        state.activeApprovalId === approval.approvalId &&
        approval.status !== "pending"
          ? (pending[0]?.approvalId ?? null)
          : (state.activeApprovalId ?? pending[0]?.approvalId ?? null);
      return {
        pending,
        activeApprovalId,
        sheetOpen: pending.length > 0 && state.sheetOpen,
      };
    });
  },
  removeApproval: (approvalId) => {
    set((state) => {
      const pending = state.pending.filter((a) => a.approvalId !== approvalId);
      return {
        pending,
        activeApprovalId:
          state.activeApprovalId === approvalId
            ? (pending[0]?.approvalId ?? null)
            : state.activeApprovalId,
        sheetOpen: pending.length > 0 && state.sheetOpen,
      };
    });
  },
  openSheet: (approvalId) => {
    set((state) => ({
      sheetOpen: true,
      activeApprovalId:
        approvalId ?? state.activeApprovalId ?? state.pending[0]?.approvalId ?? null,
    }));
  },
  closeSheet: () => {
    // The sheet is non-dismissable while pendings exist; callers use this
    // only after the queue drains or for preview cleanup.
    set((state) => ({
      sheetOpen: state.pending.length > 0 ? true : false,
      activeApprovalId:
        state.pending.length > 0 ? state.activeApprovalId : null,
    }));
  },
  setActive: (activeApprovalId) => {
    set({ activeApprovalId });
  },
  setTaskMode: (taskId, mode) => {
    set((state) => ({ taskModes: { ...state.taskModes, [taskId]: mode } }));
  },
  setPlanDocument: (taskId, doc) => {
    set((state) => ({
      planDocuments: { ...state.planDocuments, [taskId]: doc },
    }));
  },
  setCompensation: (execution) => {
    set((state) => ({
      compensations: { ...state.compensations, [execution.planId]: execution },
    }));
  },
  setError: (lastError) => {
    set({ lastError });
  },
}));

/** Currently focused approval (active id first, else queue head). */
export function selectActiveApproval(state: ApprovalState): ApprovalRequest | null {
  if (state.activeApprovalId !== null) {
    const found = state.pending.find(
      (a) => a.approvalId === state.activeApprovalId,
    );
    if (found) return found;
  }
  return state.pending[0] ?? null;
}

/** Pending count for the status bar / Void Neon gate. */
export function selectPendingCount(state: ApprovalState): number {
  return state.pending.length;
}
