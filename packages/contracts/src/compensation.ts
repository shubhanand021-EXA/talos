import { z } from "zod";
import {
  CompensationPlanIdSchema,
  IsoDateTimeSchema,
  TalosIdSchema,
  TaskIdSchema,
} from "./identity.js";

export const CompensationStrategySchema = z.enum([
  "backup_restore",
  "snapshot_revert",
  "rollback_command",
  "manual_review",
]);
export type CompensationStrategy = z.infer<typeof CompensationStrategySchema>;

export const CompensationStepSchema = z
  .object({
    id: TalosIdSchema,
    strategy: CompensationStrategySchema,
    description: z.string().min(1).max(1024),
    command: z.string().min(1).max(2048).optional(),
    target: z.string().min(1).max(1024).optional(),
    estimatedSeconds: z.number().int().nonnegative().max(86400).optional(),
  })
  .strict();
export type CompensationStep = z.infer<typeof CompensationStepSchema>;

export const CompensationPlanSchema = z
  .object({
    id: CompensationPlanIdSchema,
    actionRequestId: TalosIdSchema.optional(),
    taskId: TaskIdSchema.optional(),
    reason: z.string().min(1).max(2048),
    steps: z.array(CompensationStepSchema).min(1).max(32),
    createdAt: IsoDateTimeSchema,
  })
  .strict();
export type CompensationPlan = z.infer<typeof CompensationPlanSchema>;
