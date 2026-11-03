import { z } from "zod";
import {
  GATEWAY_HEARTBEAT_INTERVAL_MS,
  GATEWAY_HEARTBEAT_TIMEOUT_MS,
  GATEWAY_PROTOCOL_VERSION,
  GATEWAY_URL,
  GatewayCommandKind,
  GatewayEventKind,
  GatewayCommandSchema,
  GatewayEventSchema,
  gatewayCommand,
  type GatewayCommand,
  type GatewayEvent,
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
