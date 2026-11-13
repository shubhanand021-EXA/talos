import { z } from "zod";

/**
 * Talos Gateway protocol (Batch 5): strict JSON-RPC-like message schema
 * shared by the headless `talos-gateway` daemon (Rust) and every client
 * (Tauri shell, UI bridge, CLI).
 *
 * Transport: WebSocket `ws://127.0.0.1:8080/talos`.
 * Every frame is a single JSON object matching `GatewayFrameSchema`
 * (either a `Command` request or an `Event` broadcast).
 */

export const GATEWAY_URL = "ws://127.0.0.1:8080/talos" as const;
export const GATEWAY_PROTOCOL_VERSION = "talos-gateway/1" as const;
/**
 * Handshake authentication (mirrors `talos_gateway::protocol`):
 * every upgrade must carry an allowlisted `Origin` header and the per-start
 * daemon token as a `talos-gw-token.*` subprotocol element. The token NEVER
 * travels in the URL query string.
 */
export const GATEWAY_SUBPROTOCOL = "talos-gateway-1" as const;
export const GATEWAY_TOKEN_SUBPROTOCOL_PREFIX = "talos-gw-token." as const;
export const GATEWAY_ALLOWED_ORIGINS = [
  "tauri://localhost",
  "https://tauri.localhost",
  "http://localhost:5173",
  "http://127.0.0.1:5173",
] as const;
/** Heartbeat interval the daemon expects (ms). Miss 3 pongs => DISCONNECTED. */
export const GATEWAY_HEARTBEAT_INTERVAL_MS = 5_000 as const;
export const GATEWAY_HEARTBEAT_TIMEOUT_MS = 15_000 as const;

/** Commands a client may send to the daemon. */
export const GatewayCommandKind = {
  SubmitTask: "SubmitTask",
  CancelTask: "CancelTask",
  ApprovePolicy: "ApprovePolicy",
  Ping: "Ping",
  GetStatus: "GetStatus",
  ForwardIpc: "ForwardIpc",
} as const;
export type GatewayCommandKind =
  (typeof GatewayCommandKind)[keyof typeof GatewayCommandKind];

export const GatewayCommandKindSchema = z.enum([
  GatewayCommandKind.SubmitTask,
  GatewayCommandKind.CancelTask,
  GatewayCommandKind.ApprovePolicy,
  GatewayCommandKind.Ping,
  GatewayCommandKind.GetStatus,
  GatewayCommandKind.ForwardIpc,
]);

/** Payload for `SubmitTask`: durable queue submission through the daemon. */
export const GatewaySubmitTaskSchema = z
  .object({
    title: z.string().min(1).max(256),
    description: z.string().max(4096).nullish(),
    priority: z.enum(["low", "normal", "high", "critical"]).default("normal"),
    scenario: z.string().min(1).max(64).default("quick_success"),
  })
  .strict();
export type GatewaySubmitTask = z.infer<typeof GatewaySubmitTaskSchema>;

/** Payload for `CancelTask`. */
export const GatewayCancelTaskSchema = z
  .object({ taskId: z.string().min(1).max(128) })
  .strict();
export type GatewayCancelTask = z.infer<typeof GatewayCancelTaskSchema>;

/** Payload for `ApprovePolicy` (human grant/deny of a pending approval). */
export const GatewayApprovePolicySchema = z
  .object({
    approvalId: z.string().min(1).max(128),
    approved: z.boolean(),
    reason: z.string().max(2048).nullish(),
  })
  .strict();
export type GatewayApprovePolicy = z.infer<typeof GatewayApprovePolicySchema>;

/**
 * Generic IPC forward: lets the Tauri shell route any of its 106 typed IPC
 * commands through the daemon over the same socket without a second channel.
 */
export const GatewayForwardIpcSchema = z
  .object({
    command: z.string().min(1).max(128),
    args: z.record(z.string(), z.unknown()).default({}),
  })
  .strict();
export type GatewayForwardIpc = z.infer<typeof GatewayForwardIpcSchema>;

/** One command frame: `{ protocol, id, command, payload }`. */
export const GatewayCommandSchema = z
  .object({
    protocol: z.literal(GATEWAY_PROTOCOL_VERSION),
    id: z.string().min(1).max(128),
    command: GatewayCommandKindSchema,
    payload: z.unknown(),
  })
  .strict();
export type GatewayCommand = z.infer<typeof GatewayCommandSchema>;

/** Events the daemon broadcasts to every connected client. */
export const GatewayEventKind = {
  TaskStarted: "TaskStarted",
  TaskCompleted: "TaskCompleted",
  ToolInvoked: "ToolInvoked",
  EvidenceRecorded: "EvidenceRecorded",
  PolicyRequired: "PolicyRequired",
  CaptchaDetected: "CaptchaDetected",
  AuthRequired: "AuthRequired",
  Pong: "Pong",
  Error: "Error",
  SchedulesListed: "SchedulesListed",
} as const;
export type GatewayEventKind =
  (typeof GatewayEventKind)[keyof typeof GatewayEventKind];

export const GatewayEventKindSchema = z.enum([
  GatewayEventKind.TaskStarted,
  GatewayEventKind.TaskCompleted,
  GatewayEventKind.ToolInvoked,
  GatewayEventKind.EvidenceRecorded,
  GatewayEventKind.PolicyRequired,
  GatewayEventKind.CaptchaDetected,
  GatewayEventKind.AuthRequired,
  GatewayEventKind.Pong,
  GatewayEventKind.Error,
  GatewayEventKind.SchedulesListed,
]);

/** One event frame: `{ protocol, event, payload, taskId?, at }`. */
export const GatewayEventSchema = z
  .object({
    protocol: z.literal(GATEWAY_PROTOCOL_VERSION),
    event: GatewayEventKindSchema,
    payload: z.unknown(),
    taskId: z.string().min(1).max(128).nullish(),
    at: z.string().min(1),
  })
  .strict();
export type GatewayEvent = z.infer<typeof GatewayEventSchema>;

/** Either direction of the socket: commands go up, events come down. */
export const GatewayFrameSchema = z.union([
  GatewayCommandSchema,
  GatewayEventSchema,
]);
export type GatewayFrame = z.infer<typeof GatewayFrameSchema>;

/** Typed payloads per event kind (all bounded, never prompts or keys). */
export const GatewayTaskStartedPayloadSchema = z
  .object({
    taskId: z.string().min(1).max(128),
    title: z.string().min(1).max(256),
  })
  .strict();

export const GatewayTaskCompletedPayloadSchema = z
  .object({
    taskId: z.string().min(1).max(128),
    status: z.string().min(1).max(32),
  })
  .strict();

export const GatewayToolInvokedPayloadSchema = z
  .object({
    taskId: z.string().min(1).max(128),
    toolId: z.string().min(1).max(128),
    dryRun: z.boolean(),
  })
  .strict();

export const GatewayEvidenceRecordedPayloadSchema = z
  .object({
    evidenceId: z.string().min(1).max(128),
    kind: z.string().min(1).max(128),
    taskId: z.string().min(1).max(128).nullish(),
  })
  .strict();

export const GatewayPolicyRequiredPayloadSchema = z
  .object({
    approvalId: z.string().min(1).max(128),
    taskId: z.string().min(1).max(128),
    summary: z.string().min(1).max(1024),
  })
  .strict();

/**
 * Payload for `CaptchaDetected` / `AuthRequired`: the paused browser
 * intervention the Desktop UI must surface (modal + foreground window).
 * Linkage + reason only — never page content or challenge images.
 */
export const GatewayHitlRequiredPayloadSchema = z
  .object({
    interventionId: z.string().min(1).max(128),
    taskId: z.string().min(1).max(128),
    kind: z.enum(["captcha_detected", "auth_required"]),
    url: z.string().max(2048).nullish(),
    reason: z.string().min(1).max(256),
  })
  .strict();
export type GatewayHitlRequiredPayload = z.infer<
  typeof GatewayHitlRequiredPayloadSchema
>;

export const GatewayPongPayloadSchema = z
  .object({ sequence: z.number().int().nonnegative() })
  .strict();

export const GatewayErrorPayloadSchema = z
  .object({
    code: z.string().min(1).max(64),
    message: z.string().min(1).max(2048),
    inReplyTo: z.string().min(1).max(128).nullish(),
  })
  .strict();

/** Build a command frame with the protocol tag filled in. */
export function gatewayCommand(
  id: string,
  command: GatewayCommandKind,
  payload: unknown,
): GatewayCommand {
  return { protocol: GATEWAY_PROTOCOL_VERSION, id, command, payload };
}

/** Build an event frame stamped with the current time. */
export function gatewayEvent(
  event: GatewayEventKind,
  payload: unknown,
  taskId?: string,
): GatewayEvent {
  return {
    protocol: GATEWAY_PROTOCOL_VERSION,
    event,
    payload,
    taskId: taskId ?? null,
    at: new Date().toISOString(),
  };
}

/** Validate one inbound payload for `command`, returning a typed value. */
export function parseGatewayCommandPayload(
  command: GatewayCommandKind,
  payload: unknown,
):
  | {
      ok: true;
      value:
        | GatewaySubmitTask
        | GatewayCancelTask
        | GatewayApprovePolicy
        | GatewayForwardIpc
        | null;
    }
  | { ok: false; error: string } {
  switch (command) {
    case GatewayCommandKind.SubmitTask: {
      const parsed = GatewaySubmitTaskSchema.safeParse(payload ?? {});
      return parsed.success
        ? { ok: true, value: parsed.data }
        : { ok: false, error: parsed.error.message };
    }
    case GatewayCommandKind.CancelTask: {
      const parsed = GatewayCancelTaskSchema.safeParse(payload ?? {});
      return parsed.success
        ? { ok: true, value: parsed.data }
        : { ok: false, error: parsed.error.message };
    }
    case GatewayCommandKind.ApprovePolicy: {
      const parsed = GatewayApprovePolicySchema.safeParse(payload ?? {});
      return parsed.success
        ? { ok: true, value: parsed.data }
        : { ok: false, error: parsed.error.message };
    }
    case GatewayCommandKind.ForwardIpc: {
      const parsed = GatewayForwardIpcSchema.safeParse(payload ?? {});
      return parsed.success
        ? { ok: true, value: parsed.data }
        : { ok: false, error: parsed.error.message };
    }
    case GatewayCommandKind.Ping:
    case GatewayCommandKind.GetStatus:
      return { ok: true, value: null };
  }
}
