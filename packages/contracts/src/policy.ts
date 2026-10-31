import { z } from "zod";
import {
  AgentIdSchema,
  IsoDateTimeSchema,
  TalosIdSchema,
  TaskIdSchema,
} from "./identity.js";

export const RiskLevelSchema = z.enum([
  "low",
  "medium",
  "high",
  "irreversible",
]);
export type RiskLevel = z.infer<typeof RiskLevelSchema>;

export const ActionRequestSchema = z
  .object({
    id: TalosIdSchema,
    agentId: AgentIdSchema,
    taskId: TaskIdSchema,
    capability: z.string().min(1).max(128),
    intent: z.string().min(1).max(1024),
    target: z.string().min(1).max(1024),
    payload: z.unknown(),
    riskLevel: RiskLevelSchema,
    reversible: z.boolean(),
    requestedAt: IsoDateTimeSchema,
  })
  .strict();
export type ActionRequest = z.infer<typeof ActionRequestSchema>;

export const PolicyDecisionSchema = z.enum([
  "allow",
  "deny",
  "require_evidence",
  "require_compensation",
  "require_human_review",
]);
export type PolicyDecision = z.infer<typeof PolicyDecisionSchema>;

export const PolicyEvaluationSchema = z
  .object({
    id: TalosIdSchema,
    actionRequestId: TalosIdSchema,
    decision: PolicyDecisionSchema,
    reason: z.string().min(1).max(2048),
    policyVersion: z.string().min(1).max(64),
    evaluatedAt: IsoDateTimeSchema,
  })
  .strict();
export type PolicyEvaluation = z.infer<typeof PolicyEvaluationSchema>;
