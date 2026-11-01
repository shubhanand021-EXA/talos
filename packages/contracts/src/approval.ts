import { z } from "zod";
import { IsoDateTimeSchema, TalosIdSchema, TaskIdSchema } from "./identity.js";
import { RiskLevelSchema } from "./policy.js";

export const EffectClassSchema = z.enum([
  "READ",
  "LOCAL_WRITE",
  "EXTERNAL_WRITE",
  "DESTRUCTIVE",
  "FINANCIAL",
  "PRIVILEGED",
]);
export type EffectClass = z.infer<typeof EffectClassSchema>;

export const ReversibilitySchema = z.enum([
  "reversible",
  "irreversible",
  "unknown",
]);
export type Reversibility = z.infer<typeof ReversibilitySchema>;

export const TaskModeSchema = z.enum(["plan", "act", "auto"]);
export type TaskMode = z.infer<typeof TaskModeSchema>;

export const ApprovalStatusSchema = z.enum([
  "pending",
  "granted",
  "denied",
  "expired",
  "modified",
]);
export type ApprovalStatus = z.infer<typeof ApprovalStatusSchema>;

export const ApprovalRiskLevelSchema = RiskLevelSchema;
export type ApprovalRiskLevel = z.infer<typeof RiskLevelSchema>;

export const DiffFileSchema = z
  .object({
    path: z.string().min(1).max(1024),
    original: z.string().max(200_000),
    proposed: z.string().max(200_000),
  })
  .strict();
export type DiffFile = z.infer<typeof DiffFileSchema>;

export const CostEstimateSchema = z
  .object({
    tokens: z.number().int().nonnegative(),
    estimatedUsd: z.number().min(0),
  })
  .strict();
export type CostEstimate = z.infer<typeof CostEstimateSchema>;

export const CompensationStepViewSchema = z
  .object({
    order: z.number().int().nonnegative(),
    actionType: z.string().min(1).max(128),
    description: z.string().min(1).max(1024),
    payload: z.unknown(),
    status: z.string().min(1).max(32),
  })
  .strict();
export type CompensationStepView = z.infer<typeof CompensationStepViewSchema>;

export const ApprovalRequestSchema = z
  .object({
    approvalId: TalosIdSchema,
    taskId: TaskIdSchema,
    agentId: z.string().min(1).max(128),
    actionId: z.string().min(1).max(128),
    summary: z.string().min(1).max(2048),
    target: z.string().min(1).max(1024),
    capability: z.string().min(1).max(128),
    effectClass: EffectClassSchema,
    reversibility: ReversibilitySchema,
    riskLevel: RiskLevelSchema,
    policyReason: z.string().min(1).max(2048),
    requiredChecks: z.array(z.string().min(1).max(128)).max(32),
    payload: z.unknown(),
    payloadKind: z.string().min(1).max(32),
    originalContent: z.string().max(200_000).nullish(),
    proposedContent: z.string().max(200_000).nullish(),
    diffFiles: z.array(DiffFileSchema).max(32),
    compensation: z.array(CompensationStepViewSchema).max(32),
    costEstimate: CostEstimateSchema.nullish(),
    scope: z.string().min(1).max(2048),
    expiresAt: IsoDateTimeSchema,
    requestedAt: IsoDateTimeSchema,
    status: ApprovalStatusSchema,
    decidedBy: z.string().min(1).max(128).nullish(),
    denyReason: z.string().max(2048).nullish(),
  })
  .strict();
export type ApprovalRequest = z.infer<typeof ApprovalRequestSchema>;

export const PlannedActionSchema = z
  .object({
    approvalId: TalosIdSchema,
    description: z.string().min(1).max(2048),
    target: z.string().min(1).max(1024),
    effect: EffectClassSchema,
    reversibility: ReversibilitySchema,
    risk: RiskLevelSchema,
    status: ApprovalStatusSchema,
  })
  .strict();
export type PlannedAction = z.infer<typeof PlannedActionSchema>;

export const PlanDocumentSchema = z
  .object({
    taskId: TaskIdSchema,
    mode: TaskModeSchema,
    actions: z.array(PlannedActionSchema).max(256),
    createdAt: IsoDateTimeSchema,
    approvedCount: z.number().int().nonnegative(),
    pendingCount: z.number().int().nonnegative(),
  })
  .strict();
export type PlanDocument = z.infer<typeof PlanDocumentSchema>;

export const CompensationExecutionSchema = z
  .object({
    planId: TalosIdSchema,
    taskId: TaskIdSchema,
    failureReason: z.string().min(1).max(2048),
    steps: z.array(CompensationStepViewSchema).max(32),
    stepsCompleted: z.number().int().nonnegative(),
    status: z.string().min(1).max(64),
    startedAt: IsoDateTimeSchema,
    finishedAt: IsoDateTimeSchema.nullish(),
  })
  .strict();
export type CompensationExecution = z.infer<typeof CompensationExecutionSchema>;
