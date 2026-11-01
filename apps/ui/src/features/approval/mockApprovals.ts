import { ApprovalRequestSchema, TalosIdSchema, TaskIdSchema } from "@talos/contracts";
import type {
  ApprovalRequest,
  CompensationExecution,
  PlanDocument,
} from "@talos/contracts";

/** Deterministic preview approvals for browser mode (no Tauri IPC). */
const STAMP = "2026-11-01T12:00:00.000Z";
const EXPIRY = "2026-11-01T12:05:00.000Z";

export function getMockApprovals(): ApprovalRequest[] {
  const raw: unknown[] = [
    {
      approvalId: TalosIdSchema.parse("appr-task-email-1"),
      taskId: TaskIdSchema.parse("task-email"),
      agentId: "agent-coder-1",
      actionId: "req-email-1",
      summary:
        "Send this email to 43 recipients using the account work@example.com; attachment Q3-report.pdf will leave the device",
      target: "work@example.com",
      capability: "NetworkRequest",
      effectClass: "EXTERNAL_WRITE",
      reversibility: "irreversible",
      riskLevel: "high",
      policyReason:
        "require_human_review: high-risk non-reversible NetworkRequest on work@example.com requires human review",
      requiredChecks: ["human_review"],
      payload: {
        method: "POST",
        endpoint: "https://mail.internal/send",
        recipients: Array.from({ length: 43 }, (_, i) => `user${i}@example.com`),
        account: "work@example.com",
        attachment: "Q3-report.pdf",
      },
      payloadKind: "network",
      originalContent: null,
      proposedContent: null,
      diffFiles: [],
      compensation: [
        {
          order: 0,
          actionType: "recall_email",
          description: "Attempt recall within 10 minutes of send",
          payload: { account: "work@example.com" },
          status: "pending",
        },
        {
          order: 1,
          actionType: "manual_review",
          description: "Human confirms recipients and attachment",
          payload: {},
          status: "pending",
        },
      ],
      costEstimate: { tokens: 480, estimatedUsd: 0.00096 },
      scope:
        "Grants req-email-1 one-shot permission to exercise NetworkRequest on work@example.com; valid for this task only and expires with the approval.",
      expiresAt: EXPIRY,
      requestedAt: STAMP,
      status: "pending",
      decidedBy: null,
      denyReason: null,
    },
    {
      approvalId: TalosIdSchema.parse("appr-task-code-2"),
      taskId: TaskIdSchema.parse("task-contracts"),
      agentId: "agent-coder-1",
      actionId: "req-code-2",
      summary: "Apply formatter to packages/contracts/src/policy.ts on src/policy.ts",
      target: "src/policy.ts",
      capability: "WriteFile",
      effectClass: "LOCAL_WRITE",
      reversibility: "reversible",
      riskLevel: "medium",
      policyReason:
        "require_human_review: write-like WriteFile on src/policy.ts requires recorded evidence before execution",
      requiredChecks: ["record_evidence", "evidence_verified", "human_review"],
      payload: {
        command: "apply-edit",
        path: "src/policy.ts",
        original: "export const x = 1;\n",
        proposed: "export const x: number = 1;\n",
      },
      payloadKind: "file_edit",
      originalContent: "export const x = 1;\nconsole.log(x);\n",
      proposedContent: "export const x: number = 1;\nconsole.log(x);\n",
      diffFiles: [
        {
          path: "src/policy.ts",
          original: "export const x = 1;\nconsole.log(x);\n",
          proposed: "export const x: number = 1;\nconsole.log(x);\n",
        },
      ],
      compensation: [],
      costEstimate: { tokens: 160, estimatedUsd: 0.00032 },
      scope:
        "Grants req-code-2 one-shot permission to exercise WriteFile on src/policy.ts; valid for this task only and expires with the approval.",
      expiresAt: EXPIRY,
      requestedAt: STAMP,
      status: "pending",
      decidedBy: null,
      denyReason: null,
    },
  ];
  return raw.map((a) => ApprovalRequestSchema.parse(a));
}

export function getMockPlanDocument(taskId: string): PlanDocument {
  const parsed = TaskIdSchema.safeParse(taskId);
  return {
    taskId: parsed.success ? parsed.data : TaskIdSchema.parse("task-email"),
    mode: "plan",
    actions: getMockApprovals()
      .filter((a) => a.taskId === taskId || taskId === "task-email")
      .map((a) => ({
        approvalId: a.approvalId,
        description: a.summary,
        target: a.target,
        effect: a.effectClass,
        reversibility: a.reversibility,
        risk: a.riskLevel,
        status: a.status,
      })),
    createdAt: STAMP,
    approvedCount: 0,
    pendingCount: 1,
  };
}

export function getMockCompensation(): CompensationExecution {
  return {
    planId: TalosIdSchema.parse("plan-email-1"),
    taskId: TaskIdSchema.parse("task-email"),
    failureReason: "Send partially failed: 12 of 43 deliveries bounced",
    steps: [
      {
        order: 0,
        actionType: "recall_email",
        description: "Attempt recall within 10 minutes of send",
        payload: { account: "work@example.com" },
        status: "pending",
      },
      {
        order: 1,
        actionType: "manual_review",
        description: "Human confirms recipients and attachment",
        payload: {},
        status: "pending",
      },
    ],
    stepsCompleted: 0,
    status: "pending",
    startedAt: STAMP,
    finishedAt: null,
  };
}
