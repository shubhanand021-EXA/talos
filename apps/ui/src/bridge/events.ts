import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { z } from "zod";
import {
  AgentDescriptorSchema,
  EvidenceRecordSchema,
  HeartbeatEventSchema,
  IpcEventName,
  PolicyEvaluationSchema,
  type AgentDescriptor,
  type EvidenceRecord,
  type HeartbeatEvent,
  type PolicyEvaluation,
} from "@talos/contracts";
import { isTauri } from "./client.js";
import { BridgeError } from "./errors.js";

/** Task payload carried by `taskCreated` / `taskUpdated`. */
export const DiagnosticTaskPayloadSchema = z
  .object({
    task: z
      .object({
        id: z.string().min(1).max(128),
        title: z.string().min(1).max(256),
        status: z.string().min(1).max(64),
      })
      .strict(),
  })
  .strict();
export type DiagnosticTaskPayload = z.infer<typeof DiagnosticTaskPayloadSchema>;

/** Queue task payload emitted by submit/cancel/retry (superset of diagnostic). */
export const QueueTaskPayloadSchema = z
  .object({
    task: z
      .object({
        id: z.string().min(1).max(128),
        title: z.string().min(1).max(256),
        status: z.string().min(1).max(64),
        priority: z.string().min(1).max(64).optional(),
        attempts: z.number().int().nonnegative().optional(),
        maxAttempts: z.number().int().positive().optional(),
      })
      .strict(),
  })
  .strict();
export type QueueTaskPayload = z.infer<typeof QueueTaskPayloadSchema>;

/** Payload of `agentSpawned`: a full contracts-validated agent descriptor. */
export const AgentSpawnedPayloadSchema = z
  .object({ agent: AgentDescriptorSchema })
  .strict();
export type AgentSpawnedPayload = z.infer<typeof AgentSpawnedPayloadSchema>;

/** Payload of `policyEvaluated`. */
export const PolicyEvaluatedPayloadSchema = z
  .object({ evaluation: PolicyEvaluationSchema })
  .strict();
export type PolicyEvaluatedPayload = z.infer<
  typeof PolicyEvaluatedPayloadSchema
>;

/** Payload of `evidenceUpdated`. */
export const EvidenceUpdatedPayloadSchema = z
  .object({ evidence: EvidenceRecordSchema })
  .strict();
export type EvidenceUpdatedPayload = z.infer<
  typeof EvidenceUpdatedPayloadSchema
>;

/** Subscribe to one typed Tauri event. Resolves to an unlisten function. */
export async function subscribeTalosEvent<T>(
  event: string,
  handler: (payload: T) => void,
  schema?: z.ZodType<T>,
): Promise<UnlistenFn> {
  if (!isTauri()) {
    throw new BridgeError(
      "backend_unavailable",
      `Cannot subscribe to "${event}" outside the Tauri container`,
    );
  }
  try {
    return await listen<unknown>(event, (tauriEvent) => {
      if (schema === undefined) {
        handler(tauriEvent.payload as T);
        return;
      }
      const parsed = schema.safeParse(tauriEvent.payload);
      if (parsed.success) {
        handler(parsed.data);
      }
    });
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    throw new BridgeError(
      "subscription_failed",
      `Subscribe "${event}" failed: ${message}`,
    );
  }
}

export { HeartbeatEventSchema, IpcEventName };
export type {
  AgentDescriptor,
  EvidenceRecord,
  HeartbeatEvent,
  PolicyEvaluation,
};
