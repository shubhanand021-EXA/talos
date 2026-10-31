export { BridgeError, isBackendUnavailable } from "./errors.js";
export type { BridgeErrorCode } from "./errors.js";
export { invokeCommand, isTauri } from "./client.js";
export {
  AppInfoSchema,
  ClaimSummarySchema,
  CompensationPlanSummarySchema,
  DiagnosticSummarySchema,
  DiagnosticCheckSchema,
  EventSummarySchema,
  EvidenceSummarySchema,
  LedgerStatusSchema,
  SubmitTaskInputSchema,
  TaskQueueEntrySchema,
  cancelTask,
  getAppInfo,
  getSystemStatus,
  getTaskState,
  listTasks,
  requestAction,
  retryTask,
  runDiagnostic,
  submitTask,
  verifyEvidenceLedger,
} from "./api.js";
export type {
  BackendAppInfo,
  ClaimSummary,
  CompensationPlanSummary,
  DiagnosticSummary,
  EventSummary,
  EvidenceSummary,
  HeartbeatEvent,
  LedgerStatus,
  SubmitTaskInput,
  TaskQueueEntry,
} from "./api.js";
export { HeartbeatEventSchema } from "./api.js";
export {
  AgentSpawnedPayloadSchema,
  DiagnosticTaskPayloadSchema,
  EvidenceUpdatedPayloadSchema,
  PolicyEvaluatedPayloadSchema,
  QueueTaskPayloadSchema,
  subscribeTalosEvent,
} from "./events.js";
export type {
  AgentSpawnedPayload,
  DiagnosticTaskPayload,
  EvidenceUpdatedPayload,
  PolicyEvaluatedPayload,
  QueueTaskPayload,
} from "./events.js";
export { useTalosSync } from "./useTalosSync.js";
