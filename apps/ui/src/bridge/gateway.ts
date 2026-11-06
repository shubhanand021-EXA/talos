import { z } from "zod";
import {
  ChannelStatusViewSchema,
  GATEWAY_HEARTBEAT_INTERVAL_MS,
  GATEWAY_HEARTBEAT_TIMEOUT_MS,
  GATEWAY_PROTOCOL_VERSION,
  GATEWAY_URL,
  GatewayCommandKind,
  GatewayCommandSchema,
  GatewayDaemonEventSchema,
  GatewayEventKind,
  GatewayEventSchema,
  LoggedMessageSchema,
  RemoteRunnerConfigSchema,
  RunnerStatusViewSchema,
  SchedulePolicyRequiredSchema,
  ScheduleStatusViewSchema,
  gatewayCommand,
  type ChannelStatusView,
  type GatewayCommand,
  type GatewayDaemonEvent,
  type GatewayEvent,
  type LoggedMessage,
  type RemoteRunnerConfig,
  type RunnerStatusView,
  type ScheduleStatusView,
} from "@talos/contracts";
import { BridgeError } from "./errors.js";
import { invokeCommand, isTauri } from "./client.js";

export type GatewayLink = "unknown" | "connected" | "disconnected";

export interface GatewayStatusView {
  link: GatewayLink;
  url: string;
  missedPongs: number;
  lastSequence?: number | null;
  daemonVersion?: string | null;
}

export interface GatewayClientOptions {
  url?: string;
  heartbeatIntervalMs?: number;
  onEvent?: (event: GatewayEvent) => void;
  onStatus?: (status: GatewayStatusView) => void;
}

/**
 * Headless-daemon link for the UI. The Tauri shell owns the WebSocket when
 * running natively; this client is the browser fallback (plain Chromium,
 * `pnpm dev:ui`) plus the reconnection prober both shells share:
 *
 * - `Ping` every `heartbeatIntervalMs`, expecting `Pong` within the timeout.
 * - 3 missed pongs => `disconnected` (UI renders DISCONNECTED + Restart).
 * - Exponential-backoff reconnect (1s, 2s, 4s … capped at 30s).
 */
export class GatewayClient {
  private socket: WebSocket | null = null;
  private readonly url: string;
  private readonly heartbeatIntervalMs: number;
  private readonly onEvent: ((event: GatewayEvent) => void) | undefined;
  private readonly onStatus: ((status: GatewayStatusView) => void) | undefined;
  private sequence = 0;
  private missedPongs = 0;
  private status: GatewayLink = "unknown";
  private heartbeatTimer: ReturnType<typeof setInterval> | null = null;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private reconnectDelayMs = 1000;
  private closed = false;

  constructor(options: GatewayClientOptions = {}) {
    this.url = options.url ?? GATEWAY_URL;
    this.heartbeatIntervalMs =
      options.heartbeatIntervalMs ?? GATEWAY_HEARTBEAT_INTERVAL_MS;
    this.onEvent = options.onEvent;
    this.onStatus = options.onStatus;
  }

  /** Open the socket and start heartbeats. Idempotent. */
  connect(): void {
    this.closed = false;
    if (this.socket !== null) {
      return;
    }
    let socket: WebSocket;
    try {
      socket = new WebSocket(this.url);
    } catch {
      this.markDisconnected();
      this.scheduleReconnect();
      return;
    }
    this.socket = socket;
    socket.onopen = () => {
      this.reconnectDelayMs = 1000;
      this.startHeartbeat();
    };
    socket.onmessage = (message) => {
      this.handleFrame(String(message.data));
    };
    socket.onerror = () => {
      this.markDisconnected();
    };
    socket.onclose = () => {
      this.socket = null;
      this.stopHeartbeat();
      if (!this.closed) {
        this.markDisconnected();
        this.scheduleReconnect();
      }
    };
  }

  /** Stop heartbeats and close the socket. */
  disconnect(): void {
    this.closed = true;
    if (this.reconnectTimer !== null) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
    this.stopHeartbeat();
    this.socket?.close();
    this.socket = null;
  }

  /** Current link snapshot for the status bar and DISCONNECTED banner. */
  snapshot(): GatewayStatusView {
    return {
      link: this.status,
      url: this.url,
      missedPongs: this.missedPongs,
      lastSequence: this.sequence > 0 ? this.sequence : null,
      daemonVersion: null,
    };
  }

  /** Send one command frame; resolves with the first addressed reply. */
  sendCommand(
    command: GatewayCommand["command"],
    payload: unknown,
  ): Promise<GatewayEvent> {
    const socket = this.socket;
    if (socket === null || socket.readyState !== WebSocket.OPEN) {
      return Promise.reject(
        new BridgeError("backend_unavailable", "Gateway socket is not open"),
      );
    }
    const id = `ui-${Date.now()}-${Math.floor(Math.random() * 1_000_000)}`;
    const frame = gatewayCommand(id, command, payload);
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        socket.removeEventListener("message", listener);
        reject(
          new BridgeError(
            "invoke_rejected",
            `Gateway command "${command}" timed out`,
          ),
        );
      }, GATEWAY_HEARTBEAT_TIMEOUT_MS);
      const listener = (message: MessageEvent) => {
        const event = GatewayEventSchema.safeParse(
          safeJsonParse(String(message.data)),
        );
        if (!event.success) {
          return;
        }
        const payloadValue = event.data.payload as
          { inReplyTo?: unknown } | null | undefined;
        const inReplyTo =
          typeof payloadValue === "object" && payloadValue !== null
            ? payloadValue.inReplyTo
            : undefined;
        const sequenceEcho =
          command === GatewayCommandKind.Ping &&
          event.data.event === GatewayEventKind.Pong &&
          (event.data.payload as { sequence?: unknown }).sequence ===
            (frame.payload as { sequence?: unknown }).sequence;
        if (inReplyTo === id || sequenceEcho) {
          clearTimeout(timer);
          socket.removeEventListener("message", listener);
          resolve(event.data);
        }
      };
      socket.addEventListener("message", listener);
      socket.send(JSON.stringify(frame));
    });
  }

  private handleFrame(raw: string): void {
    const parsed = GatewayCommandSchema.or(GatewayEventSchema).safeParse(
      safeJsonParse(raw),
    );
    if (!parsed.success) {
      return;
    }
    if (!("event" in parsed.data)) {
      return;
    }
    const event = parsed.data;
    // Runtime tag check (the schema type narrows the literal, so compare
    // through a widened binding to keep the wire guard).
    const tag: string = event.protocol;
    if (tag !== GATEWAY_PROTOCOL_VERSION) {
      return;
    }
    if (event.event === GatewayEventKind.Pong) {
      this.missedPongs = 0;
      if (this.status !== "connected") {
        this.status = "connected";
        this.emitStatus();
      }
    }
    this.onEvent?.(event);
  }

  private startHeartbeat(): void {
    this.stopHeartbeat();
    this.heartbeatTimer = setInterval(() => {
      this.ping();
    }, this.heartbeatIntervalMs);
    this.ping();
  }

  private stopHeartbeat(): void {
    if (this.heartbeatTimer !== null) {
      clearInterval(this.heartbeatTimer);
      this.heartbeatTimer = null;
    }
  }

  private ping(): void {
    const socket = this.socket;
    if (socket === null || socket.readyState !== WebSocket.OPEN) {
      this.markDisconnected();
      return;
    }
    this.sequence += 1;
    const frame = gatewayCommand(this.heartbeatId(), GatewayCommandKind.Ping, {
      sequence: this.sequence,
    });
    try {
      socket.send(JSON.stringify(frame));
    } catch {
      this.markDisconnected();
    }
  }

  private heartbeatId(): string {
    return `ui-hb-${this.sequence}`;
  }

  private markDisconnected(): void {
    this.missedPongs += 1;
    if (this.status !== "disconnected") {
      this.status = "disconnected";
      this.emitStatus();
    }
  }

  private scheduleReconnect(): void {
    if (this.closed || this.reconnectTimer !== null) {
      return;
    }
    const delay = Math.min(this.reconnectDelayMs, 30_000);
    this.reconnectDelayMs = Math.min(this.reconnectDelayMs * 2, 30_000);
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null;
      this.connect();
    }, delay);
  }

  private emitStatus(): void {
    this.onStatus?.(this.snapshot());
  }
}

function safeJsonParse(raw: string): unknown {
  try {
    return JSON.parse(raw) as unknown;
  } catch {
    return undefined;
  }
}

/** Bridge-side schema for the Tauri `gateway_status` / `gateway_restart` views. */
const GatewayStatusViewSchema: z.ZodType<GatewayStatusView> = z.object({
  link: z.enum(["unknown", "connected", "disconnected"]),
  url: z.string().min(1),
  missedPongs: z.number().int().nonnegative(),
  lastSequence: z.number().int().nonnegative().nullish(),
  daemonVersion: z.string().nullish(),
}) as z.ZodType<GatewayStatusView>;

/** Tauri invoke wrappers for the native path (shell owns the socket). */
export function getGatewayStatus(): Promise<GatewayStatusView> {
  if (!isTauri()) {
    return Promise.reject(
      new BridgeError(
        "backend_unavailable",
        "Gateway status requires the Tauri shell",
      ),
    );
  }
  return invokeCommand("gateway_status", undefined, GatewayStatusViewSchema);
}

export function restartGatewayDaemon(): Promise<GatewayStatusView> {
  if (!isTauri()) {
    return Promise.reject(
      new BridgeError(
        "backend_unavailable",
        "Gateway restart requires the Tauri shell",
      ),
    );
  }
  return invokeCommand("gateway_restart", undefined, GatewayStatusViewSchema);
}

/** Scheduler invokes (Tauri shell proxies to the daemon owner). */
function requireTauriShell(command: string): void {
  if (!isTauri()) {
    throw new BridgeError(
      "backend_unavailable",
      `Scheduler command "${command}" requires the Tauri shell`,
    );
  }
}

/** List owned HEARTBEAT schedules (next trigger + last outcome). */
export function listSchedules(): Promise<ScheduleStatusView[]> {
  requireTauriShell("list_schedules");
  return invokeCommand(
    "list_schedules",
    undefined,
    z.array(ScheduleStatusViewSchema),
  );
}

/** Pause or resume one schedule by id (returns the updated row). */
export function toggleSchedule(
  scheduleId: string,
  active: boolean,
): Promise<ScheduleStatusView[]> {
  requireTauriShell("toggle_schedule_active");
  return invokeCommand(
    "toggle_schedule_active",
    { scheduleId, active },
    z.array(ScheduleStatusViewSchema),
  );
}

/** Fire one schedule immediately (manual override, even when paused). */
export function triggerScheduleNow(scheduleId: string): Promise<unknown> {
  requireTauriShell("trigger_schedule_now");
  return invokeCommand("trigger_schedule_now", { scheduleId }, z.unknown());
}

/** Runner invokes (Tauri shell proxies to the daemon/desktop owners). */
function requireRunnerShell(command: string): void {
  if (!isTauri()) {
    throw new BridgeError(
      "backend_unavailable",
      `Runner command "${command}" requires the Tauri shell`,
    );
  }
}

/** List BYOC runner status rows (never key material). */
export function listRunners(): Promise<RunnerStatusView[]> {
  requireRunnerShell("list_runners");
  return invokeCommand(
    "list_runners",
    undefined,
    z.array(RunnerStatusViewSchema),
  );
}

/** Add or replace one runner definition (validated, secret-free). */
export function addRunner(
  runner: RemoteRunnerConfig,
): Promise<RunnerStatusView[]> {
  const parsed = RemoteRunnerConfigSchema.safeParse(runner);
  if (!parsed.success) {
    return Promise.reject(
      new Error(`Invalid runner config: ${parsed.error.message}`),
    );
  }
  requireRunnerShell("add_runner");
  return invokeCommand(
    "add_runner",
    { runner: parsed.data },
    z.array(RunnerStatusViewSchema),
  );
}

/** Remove one runner (idempotent; clears its task assignments). */
export function removeRunner(runnerId: string): Promise<RunnerStatusView[]> {
  requireRunnerShell("remove_runner");
  return invokeCommand(
    "remove_runner",
    { runnerId },
    z.array(RunnerStatusViewSchema),
  );
}

/** Live connectivity probe (`ssh ... true` / `docker info`). */
export function testRunner(runnerId: string): Promise<RunnerStatusView> {
  requireRunnerShell("test_runner");
  return invokeCommand("test_runner", { runnerId }, RunnerStatusViewSchema);
}

/** Assign (or clear with `null`) a task's BYOC runner. */
export function setTaskRunner(
  taskId: string,
  runnerId: string | null,
): Promise<string | null> {
  requireRunnerShell("set_task_runner");
  return invokeCommand(
    "set_task_runner",
    { taskId, runnerId },
    z.string().min(1).max(64).nullable(),
  );
}

/** Current assignment for one task, if any. */
export function getTaskRunner(taskId: string): Promise<string | null> {
  requireRunnerShell("get_task_runner");
  return invokeCommand(
    "get_task_runner",
    { taskId },
    z.string().min(1).max(64).nullable(),
  );
}

/** Channel invokes (Tauri shell proxies to the daemon owner). */
function requireChannelShell(command: string): void {
  if (!isTauri()) {
    throw new BridgeError(
      "backend_unavailable",
      `Channel command "${command}" requires the Tauri shell`,
    );
  }
}

/** List linked/enabled channel rows (never tokens). */
export function listChannels(): Promise<ChannelStatusView[]> {
  requireChannelShell("list_channels");
  return invokeCommand(
    "list_channels",
    undefined,
    z.array(ChannelStatusViewSchema),
  );
}

/** Link a channel: token travels once into the OS keychain. */
export function linkChannel(
  channel: string,
  token: string,
): Promise<ChannelStatusView[]> {
  requireChannelShell("link_channel");
  return invokeCommand(
    "link_channel",
    { channel, token },
    z.array(ChannelStatusViewSchema),
  );
}

/** Unlink a channel: drop credentials, disable, detach (idempotent). */
export function unlinkChannel(channel: string): Promise<ChannelStatusView[]> {
  requireChannelShell("unlink_channel");
  return invokeCommand(
    "unlink_channel",
    { channel },
    z.array(ChannelStatusViewSchema),
  );
}

/** Recent channel traffic for the UI log (metadata + excerpts). */
export function listChannelMessages(limit = 20): Promise<LoggedMessage[]> {
  requireChannelShell("list_channel_messages");
  return invokeCommand(
    "list_channel_messages",
    { limit },
    z.array(LoggedMessageSchema),
  );
}

export type {
  ChannelStatusView,
  GatewayDaemonEvent,
  LoggedMessage,
  RemoteRunnerConfig,
  RunnerStatusView,
  ScheduleStatusView,
};

/** Validate one `gatewayDaemonEvent` shell-forwarded broadcast frame. */
export function parseDaemonEvent(raw: unknown): GatewayDaemonEvent | null {
  const parsed = GatewayDaemonEventSchema.safeParse(raw);
  return parsed.success ? parsed.data : null;
}

/** Validate a scheduler `PolicyRequired` desktop notification payload. */
export function parseScheduleNotification(
  payload: unknown,
): { approvalId: string; taskId: string; summary: string } | null {
  const parsed = SchedulePolicyRequiredSchema.safeParse(payload);
  return parsed.success ? parsed.data : null;
}
