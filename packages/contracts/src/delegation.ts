import { z } from "zod";
import { TaskIdSchema } from "./identity.js";
import { ExecutionFidelitySchema } from "./fidelity.js";

/** Specialist role a sub-task is assigned to. */
export const SpecialistRoleSchema = z.enum(["coder", "browser", "researcher"]);
export type SpecialistRole = z.infer<typeof SpecialistRoleSchema>;

/** One structured artifact a specialist reports (never a conversation dump). */
export const ArtifactRefSchema = z
  .object({
    id: z.string().min(1).max(128),
    kind: z.string().min(1).max(64),
    summary: z.string().min(1).max(2048),
    hash: z.string().max(256).nullish(),
    evidenceId: z.string().min(1).max(128).nullish(),
    executionFidelity: ExecutionFidelitySchema,
  })
  .strict();
export type ArtifactRef = z.infer<typeof ArtifactRefSchema>;

/** Supervisor → specialist assignment contract. */
export const DelegationRequestSchema = z
  .object({
    parentTaskId: TaskIdSchema,
    childTaskId: TaskIdSchema,
    agentRole: SpecialistRoleSchema,
    scope: z.string().min(1).max(2048),
    toolAllowlist: z.array(z.string().min(1).max(64)).min(1).max(32),
    contextBudget: z.number().int().min(1).max(128_000),
    deadlineMs: z.number().int().min(100).max(3_600_000),
    depth: z.number().int().min(1).max(2),
  })
  .strict();
export type DelegationRequest = z.infer<typeof DelegationRequestSchema>;

/** Specialist → Supervisor completion contract. */
export const DelegationResultSchema = z
  .object({
    childTaskId: TaskIdSchema,
    status: z.enum(["completed", "failed", "canceled", "timed_out"]),
    artifacts: z.array(ArtifactRefSchema).max(32),
    evidenceIds: z.array(z.string().min(1).max(128)).max(64),
    limitations: z.array(z.string().max(1024)).max(16),
    remainingRisks: z.array(z.string().max(1024)).max(16),
    executionFidelity: ExecutionFidelitySchema,
  })
  .strict();
export type DelegationResult = z.infer<typeof DelegationResultSchema>;

/** Input for `delegate_task` (parent + optional decomposition hints). */
export const DelegateTaskInputSchema = z
  .object({
    parentTaskId: TaskIdSchema,
    goal: z.string().min(1).max(2048).optional(),
    role: SpecialistRoleSchema.optional(),
    scope: z.string().min(1).max(2048).optional(),
    deadlineMs: z.number().int().min(100).max(3_600_000).optional(),
    contextBudget: z.number().int().min(1).max(128_000).optional(),
  })
  .strict();
export type DelegateTaskInput = z.infer<typeof DelegateTaskInputSchema>;

/** Outcome of `delegate_task`: parent plus spawned children and results. */
export const DelegateTaskOutputSchema = z
  .object({
    parent: z.unknown(),
    children: z.array(z.unknown()),
    results: z.array(DelegationResultSchema),
    mergedStatus: z.string().min(1).max(32),
  })
  .strict();
export type DelegateTaskOutput = z.infer<typeof DelegateTaskOutputSchema>;

/** How a DAG plan was produced (`llm` | `keyword` fallback). */
export const PlanningMethodSchema = z.enum(["llm", "keyword"]);
export type PlanningMethod = z.infer<typeof PlanningMethodSchema>;

/** One planned DAG step for the plan viewer. */
export const PlanStepViewSchema = z
  .object({
    stepId: z.string().min(1).max(64),
    role: SpecialistRoleSchema,
    goal: z.string().min(1).max(2048),
    dependsOn: z.array(z.string().min(1).max(64)).max(32),
    toolsNeeded: z.array(z.string().min(1).max(64)).max(32),
    requiresApproval: z.boolean(),
    estimatedTokens: z.number().int().min(0).max(128_000),
  })
  .strict();
export type PlanStepView = z.infer<typeof PlanStepViewSchema>;

/** Outcome of `preview_plan`: the DAG without executing it. */
export const PlanPreviewViewSchema = z
  .object({
    parentTaskId: TaskIdSchema,
    method: PlanningMethodSchema,
    steps: z.array(PlanStepViewSchema).max(32),
    levels: z.array(z.array(z.string().min(1).max(64)).max(32)).max(32),
    requiresApproval: z.boolean(),
    fallbackReason: z.string().max(1024).nullish(),
    valid: z.boolean(),
  })
  .strict();
export type PlanPreviewView = z.infer<typeof PlanPreviewViewSchema>;

/** Planning settings surfaced by `get_planning_config`. */
export const PlanningConfigViewSchema = z
  .object({
    model: z.string().min(1).max(128),
    timeoutMs: z.number().int().min(1000).max(120_000),
    fallbackEnabled: z.boolean(),
  })
  .strict();
export type PlanningConfigView = z.infer<typeof PlanningConfigViewSchema>;

/** Input for `preview_plan`. */
export const PreviewPlanInputSchema = z
  .object({
    parentTaskId: TaskIdSchema,
    goal: z.string().min(1).max(2048).optional(),
  })
  .strict();
export type PreviewPlanInput = z.infer<typeof PreviewPlanInputSchema>;

/** Input for `update_planning_config`. */
export const UpdatePlanningConfigInputSchema = PlanningConfigViewSchema;
export type UpdatePlanningConfigInput = z.infer<
  typeof UpdatePlanningConfigInputSchema
>;
