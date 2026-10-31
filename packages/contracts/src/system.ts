import { z } from "zod";
import { AgentIdSchema, IsoDateTimeSchema, TalosIdSchema } from "./identity.js";

export const SystemStatusSchema = z
  .object({
    version: z.string().min(1).max(64),
    uptimeSeconds: z.number().nonnegative(),
    activeAgents: z.number().int().nonnegative(),
    activeTasks: z.number().int().nonnegative(),
    policyBrokerHealthy: z.boolean(),
    evidenceLedgerHealthy: z.boolean(),
    lastAuditAt: IsoDateTimeSchema.nullish(),
  })
  .strict();
export type SystemStatus = z.infer<typeof SystemStatusSchema>;

export const HeartbeatEventSchema = z
  .object({
    source: z.string().min(1).max(128),
    timestamp: IsoDateTimeSchema,
    sequence: z.number().int().nonnegative(),
    agentId: AgentIdSchema.optional(),
    uptimeSeconds: z.number().nonnegative().optional(),
  })
  .strict();
export type HeartbeatEvent = z.infer<typeof HeartbeatEventSchema>;

export const SceneNodeKindSchema = z.enum([
  "agent",
  "task",
  "policy",
  "evidence",
  "system",
  "custom",
]);
export type SceneNodeKind = z.infer<typeof SceneNodeKindSchema>;

export const SceneNodeStatusSchema = z.enum([
  "active",
  "idle",
  "warning",
  "error",
  "success",
]);
export type SceneNodeStatus = z.infer<typeof SceneNodeStatusSchema>;

export const SceneNodeSchema = z
  .object({
    id: TalosIdSchema,
    label: z.string().min(1).max(256),
    kind: SceneNodeKindSchema,
    status: SceneNodeStatusSchema,
    position: z
      .object({
        x: z.number(),
        y: z.number(),
        z: z.number().optional(),
      })
      .strict()
      .optional(),
    metadata: z.record(z.string(), z.unknown()).optional(),
  })
  .strict();
export type SceneNode = z.infer<typeof SceneNodeSchema>;

export const SceneEdgeStateSchema = z.enum([
  "active",
  "pending",
  "blocked",
  "completed",
  "failed",
]);
export type SceneEdgeState = z.infer<typeof SceneEdgeStateSchema>;

export const SceneEdgeSchema = z
  .object({
    id: TalosIdSchema,
    source: TalosIdSchema,
    target: TalosIdSchema,
    label: z.string().min(1).max(256),
    state: SceneEdgeStateSchema,
  })
  .strict();
export type SceneEdge = z.infer<typeof SceneEdgeSchema>;

export const AuditSeveritySchema = z.enum(["info", "warning", "critical"]);
export type AuditSeverity = z.infer<typeof AuditSeveritySchema>;

export const AuditReportSummarySchema = z
  .object({
    id: TalosIdSchema,
    severity: AuditSeveritySchema,
    title: z.string().min(1).max(256),
    summary: z.string().min(1).max(4096),
    findingsCount: z.number().int().nonnegative(),
    passed: z.number().int().nonnegative(),
    failed: z.number().int().nonnegative(),
    createdAt: IsoDateTimeSchema,
  })
  .strict();
export type AuditReportSummary = z.infer<typeof AuditReportSummarySchema>;
