import { z } from "zod";

/** ISO-8601 datetime string (e.g. `2026-10-31T00:00:00.000Z`). */
export const IsoDateTimeSchema = z.iso.datetime();
export type IsoDateTime = z.infer<typeof IsoDateTimeSchema>;

/** Base branded identity string. All domain IDs are non-empty strings. */
export const TalosIdSchema = z.string().min(1).max(128).brand<"TalosId">();
export type TalosId = z.infer<typeof TalosIdSchema>;

export const AgentIdSchema = z.string().min(1).max(128).brand<"AgentId">();
export type AgentId = z.infer<typeof AgentIdSchema>;

export const TaskIdSchema = z.string().min(1).max(128).brand<"TaskId">();
export type TaskId = z.infer<typeof TaskIdSchema>;

export const SessionIdSchema = z.string().min(1).max(128).brand<"SessionId">();
export type SessionId = z.infer<typeof SessionIdSchema>;

export const EvidenceIdSchema = z
  .string()
  .min(1)
  .max(128)
  .brand<"EvidenceId">();
export type EvidenceId = z.infer<typeof EvidenceIdSchema>;

export const CompensationPlanIdSchema = z
  .string()
  .min(1)
  .max(128)
  .brand<"CompensationPlanId">();
export type CompensationPlanId = z.infer<typeof CompensationPlanIdSchema>;

/** Parse helpers — throw on invalid input. */
export function parseIsoDateTime(value: unknown): IsoDateTime {
  return IsoDateTimeSchema.parse(value);
}

export function parseAgentId(value: unknown): AgentId {
  return AgentIdSchema.parse(value);
}

export function parseTaskId(value: unknown): TaskId {
  return TaskIdSchema.parse(value);
}

export function parseSessionId(value: unknown): SessionId {
  return SessionIdSchema.parse(value);
}

export function parseEvidenceId(value: unknown): EvidenceId {
  return EvidenceIdSchema.parse(value);
}

export function parseCompensationPlanId(value: unknown): CompensationPlanId {
  return CompensationPlanIdSchema.parse(value);
}
