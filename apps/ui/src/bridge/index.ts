export { BridgeError, isBackendUnavailable } from "./errors.js";
export type { BridgeErrorCode } from "./errors.js";
export { invokeCommand, isTauri } from "./client.js";
export {
  AppInfoSchema,
  DiagnosticSummarySchema,
  DiagnosticCheckSchema,
  LedgerStatusSchema,
  getAppInfo,
  getSystemStatus,
  requestAction,
  runDiagnostic,
  verifyEvidenceLedger,
} from "./api.js";
export type {
  BackendAppInfo,
  DiagnosticSummary,
  HeartbeatEvent,
  LedgerStatus,
} from "./api.js";
export { HeartbeatEventSchema } from "./api.js";
export {
  AgentSpawnedPayloadSchema,
  DiagnosticTaskPayloadSchema,
  EvidenceUpdatedPayloadSchema,
  PolicyEvaluatedPayloadSchema,
  subscribeTalosEvent,
} from "./events.js";
export type {
  AgentSpawnedPayload,
  DiagnosticTaskPayload,
  EvidenceUpdatedPayload,
  PolicyEvaluatedPayload,
} from "./events.js";
export { useTalosSync } from "./useTalosSync.js";
