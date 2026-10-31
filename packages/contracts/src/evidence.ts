import { z } from "zod";
import {
  AgentIdSchema,
  EvidenceIdSchema,
  IsoDateTimeSchema,
  SessionIdSchema,
  TalosIdSchema,
  TaskIdSchema,
} from "./identity.js";

export const EvidenceKindSchema = z.enum([
  "log",
  "file_hash",
  "screenshot",
  "terminal_output",
  "command_result",
  "state_snapshot",
  "test_result",
  "metric",
]);
export type EvidenceKind = z.infer<typeof EvidenceKindSchema>;

export const EvidenceStatusSchema = z.enum([
  "pending",
  "verified",
  "rejected",
  "conflicting",
]);
export type EvidenceStatus = z.infer<typeof EvidenceStatusSchema>;

export const EvidenceRecordSchema = z
  .object({
    id: EvidenceIdSchema,
    taskId: TaskIdSchema.optional(),
    agentId: AgentIdSchema.optional(),
    sessionId: SessionIdSchema.optional(),
    kind: EvidenceKindSchema,
    status: EvidenceStatusSchema,
    label: z.string().min(1).max(256).optional(),
    summary: z.string().max(2048).optional(),
    hash: z.string().min(1).max(256).optional(),
    uri: z.string().min(1).max(2048).optional(),
    mimeType: z.string().min(1).max(128).optional(),
    payload: z.unknown(),
    createdAt: IsoDateTimeSchema,
    verifiedAt: IsoDateTimeSchema.optional(),
    metadata: z.record(z.string(), z.unknown()).optional(),
  })
  .strict();
export type EvidenceRecord = z.infer<typeof EvidenceRecordSchema>;

export const ClaimRecordSchema = z
  .object({
    id: TalosIdSchema,
    statement: z.string().min(1).max(4096),
    evidenceIds: z.array(EvidenceIdSchema).min(1).max(64),
    createdByAgentId: AgentIdSchema.optional(),
    taskId: TaskIdSchema.optional(),
    createdAt: IsoDateTimeSchema,
  })
  .strict();
export type ClaimRecord = z.infer<typeof ClaimRecordSchema>;
