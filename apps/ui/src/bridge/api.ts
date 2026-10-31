import { z } from "zod";
import {
  ActionRequestSchema,
  HeartbeatEventSchema,
  PolicyEvaluationSchema,
  SystemStatusSchema,
  type ActionRequest,
  type HeartbeatEvent,
  type PolicyEvaluation,
  type SystemStatus,
} from "@talos/contracts";
import { invokeCommand } from "./client.js";

/** Product metadata returned by `get_app_info`. */
export const AppInfoSchema = z
  .object({
    name: z.string().min(1).max(64),
    version: z.string().min(1).max(64),
    environment: z.enum(["local", "staging", "production"]),
  })
  .strict();
export type BackendAppInfo = z.infer<typeof AppInfoSchema>;

/** Ledger snapshot returned by `verify_evidence_ledger`. */
export const LedgerStatusSchema = z
  .object({
    healthy: z.boolean(),
    event_count: z.number().int().nonnegative(),
    evidence_count: z.number().int().nonnegative(),
    claim_count: z.number().int().nonnegative(),
    detail: z.string().max(1024),
    checked_at: z.string().min(1),
  })
  .strict();
export type LedgerStatus = z.infer<typeof LedgerStatusSchema>;

/** One step of the diagnostic run. */
export const DiagnosticCheckSchema = z
  .object({
    name: z.string().min(1).max(128),
    ok: z.boolean(),
    detail: z.string().max(2048),
  })
  .strict();

/** Summary returned by `run_diagnostic`. */
export const DiagnosticSummarySchema = z
  .object({
    total: z.number().int().nonnegative(),
    passed: z.number().int().nonnegative(),
    checks: z.array(DiagnosticCheckSchema).max(32),
    ledger: LedgerStatusSchema,
    task_id: z.string().min(1).max(128),
    agent_id: z.string().min(1).max(128),
    evaluation: PolicyEvaluationSchema,
  })
  .strict();
export type DiagnosticSummary = z.infer<typeof DiagnosticSummarySchema>;

/** Fetch static product metadata. */
export function getAppInfo(): Promise<BackendAppInfo> {
  return invokeCommand("get_app_info", undefined, AppInfoSchema);
}

/** Fetch the current system health snapshot. */
export function getSystemStatus(): Promise<SystemStatus> {
  return invokeCommand("get_system_status", undefined, SystemStatusSchema);
}

/**
 * Submit one action for policy evaluation. Input is validated against the
 * contracts schema first; malformed input never reaches the backend.
 */
export function requestAction(input: ActionRequest): Promise<PolicyEvaluation> {
  const parsed = ActionRequestSchema.safeParse(input);
  if (!parsed.success) {
    return Promise.reject(
      new Error(`Invalid action request: ${parsed.error.message}`),
    );
  }
  return invokeCommand(
    "request_action",
    { raw: parsed.data },
    PolicyEvaluationSchema,
  );
}

/** Run the local echo-agent diagnostic loop. */
export function runDiagnostic(): Promise<DiagnosticSummary> {
  return invokeCommand("run_diagnostic", undefined, DiagnosticSummarySchema);
}

/** Verify ledger integrity (counts + hash probe). */
export function verifyEvidenceLedger(): Promise<LedgerStatus> {
  return invokeCommand("verify_evidence_ledger", undefined, LedgerStatusSchema);
}

export type { HeartbeatEvent };
export { HeartbeatEventSchema };
