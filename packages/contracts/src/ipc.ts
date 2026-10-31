import { z } from "zod";

export const IpcCommandName = {
  getAppInfo: "getAppInfo",
  getSystemStatus: "getSystemStatus",
  requestAction: "requestAction",
  runDiagnostic: "runDiagnostic",
  verifyEvidenceLedger: "verifyEvidenceLedger",
  getAuditSummary: "getAuditSummary",
  submitTask: "submitTask",
  cancelTask: "cancelTask",
  retryTask: "retryTask",
  listTasks: "listTasks",
  getTaskState: "getTaskState",
  listTools: "listTools",
  describeTool: "describeTool",
  invokeToolDryRun: "invokeToolDryRun",
  invokeTool: "invokeTool",
  computerSafetyStatus: "computerSafetyStatus",
  computerEmergencyStop: "computerEmergencyStop",
  computerResetStop: "computerResetStop",
  contextBudgetStatus: "contextBudgetStatus",
  contextCompact: "contextCompact",
  contextDiagnosticSnapshot: "contextDiagnosticSnapshot",
} as const;
export type IpcCommandName =
  (typeof IpcCommandName)[keyof typeof IpcCommandName];

export const IpcCommandNameSchema = z.enum([
  IpcCommandName.getAppInfo,
  IpcCommandName.getSystemStatus,
  IpcCommandName.requestAction,
  IpcCommandName.runDiagnostic,
  IpcCommandName.verifyEvidenceLedger,
  IpcCommandName.getAuditSummary,
  IpcCommandName.submitTask,
  IpcCommandName.cancelTask,
  IpcCommandName.retryTask,
  IpcCommandName.listTasks,
  IpcCommandName.getTaskState,
  IpcCommandName.listTools,
  IpcCommandName.describeTool,
  IpcCommandName.invokeToolDryRun,
  IpcCommandName.invokeTool,
  IpcCommandName.computerSafetyStatus,
  IpcCommandName.computerEmergencyStop,
  IpcCommandName.computerResetStop,
  IpcCommandName.contextBudgetStatus,
  IpcCommandName.contextCompact,
  IpcCommandName.contextDiagnosticSnapshot,
]);
export type IpcCommand = z.infer<typeof IpcCommandNameSchema>;

export const IpcEventName = {
  heartbeat: "heartbeat",
  taskCreated: "taskCreated",
  taskUpdated: "taskUpdated",
  agentSpawned: "agentSpawned",
  agentUpdated: "agentUpdated",
  policyEvaluated: "policyEvaluated",
  evidenceUpdated: "evidenceUpdated",
  compensationRequired: "compensationRequired",
  auditCompleted: "auditCompleted",
  computerSafetyChanged: "computerSafetyChanged",
  contextCompacted: "contextCompacted",
} as const;
export type IpcEventName = (typeof IpcEventName)[keyof typeof IpcEventName];

export const IpcEventNameSchema = z.enum([
  IpcEventName.heartbeat,
  IpcEventName.taskCreated,
  IpcEventName.taskUpdated,
  IpcEventName.agentSpawned,
  IpcEventName.agentUpdated,
  IpcEventName.policyEvaluated,
  IpcEventName.evidenceUpdated,
  IpcEventName.compensationRequired,
  IpcEventName.auditCompleted,
  IpcEventName.computerSafetyChanged,
  IpcEventName.contextCompacted,
]);
export type IpcEvent = z.infer<typeof IpcEventNameSchema>;
