import { beforeEach, describe, expect, it, vi } from "vitest";

const { mockInvoke } = vi.hoisted(() => ({ mockInvoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke: mockInvoke }));

import {
  GatewayClient,
  getGatewayStatus,
  restartGatewayDaemon,
} from "./gateway.js";

class FakeSocket {
  static instances: FakeSocket[] = [];
  static OPEN = 1;
  readyState = 1;
  onopen: (() => void) | null = null;
  onmessage: ((message: { data: string }) => void) | null = null;
  onerror: (() => void) | null = null;
  onclose: (() => void) | null = null;
  sent: string[] = [];
  listeners = new Map<string, Set<(message: { data: string }) => void>>();

  constructor() {
    FakeSocket.instances.push(this);
  }

  send(data: string): void {
    this.sent.push(data);
  }

  close(): void {
    this.readyState = 3;
    this.onclose?.();
  }

  addEventListener(
    type: string,
    listener: (message: { data: string }) => void,
  ): void {
    let set = this.listeners.get(type);
    if (set === undefined) {
      set = new Set();
      this.listeners.set(type, set);
    }
    set.add(listener);
  }

  removeEventListener(
    type: string,
    listener: (message: { data: string }) => void,
  ): void {
    this.listeners.get(type)?.delete(listener);
  }

  open(): void {
    this.onopen?.();
  }

  deliver(data: string): void {
    const message = { data };
    this.onmessage?.(message);
    for (const listener of this.listeners.get("message") ?? []) {
      listener(message);
    }
  }
}

beforeEach(() => {
  vi.unstubAllGlobals();
  mockInvoke.mockReset();
  FakeSocket.instances = [];
  vi.stubGlobal("WebSocket", FakeSocket);
});

function firstSocket(): FakeSocket {
  const socket = FakeSocket.instances[0];
  if (socket === undefined) {
    throw new Error("expected a gateway socket");
  }
  return socket;
}

describe("gateway client reconnection", () => {
  it("marks disconnected when the socket cannot open", () => {
    function BrokenSocket(this: unknown): never {
      throw new Error("no gateway");
    }
    vi.stubGlobal("WebSocket", BrokenSocket);
    const seen: string[] = [];
    const client = new GatewayClient({
      onStatus: (status) => seen.push(status.link),
    });
    client.connect();
    expect(client.snapshot().link).toBe("disconnected");
    expect(seen).toContain("disconnected");
    client.disconnect();
  });

  it("schedules a reconnect after close", () => {
    vi.useFakeTimers();
    try {
      const client = new GatewayClient();
      client.connect();
      const socket = firstSocket();
      socket.open();
      expect(FakeSocket.instances).toHaveLength(1);
      socket.close();
      expect(client.snapshot().link).toBe("disconnected");
      vi.advanceTimersByTime(1_000);
      expect(FakeSocket.instances.length).toBeGreaterThan(1);
      client.disconnect();
    } finally {
      vi.useRealTimers();
    }
  });

  it("resolves Ping against the echoed sequence, skipping hello ticks", async () => {
    const client = new GatewayClient();
    client.connect();
    const socket = firstSocket();
    socket.open();
    const pending = client.sendCommand("Ping", { sequence: 7 });
    // Hello greeting + server heartbeat tick arrive first and must be skipped.
    socket.deliver(
      JSON.stringify({
        protocol: "talos-gateway/1",
        event: "Pong",
        payload: { sequence: 0, status: {} },
        at: new Date().toISOString(),
      }),
    );
    socket.deliver(
      JSON.stringify({
        protocol: "talos-gateway/1",
        event: "Pong",
        payload: { sequence: 99 },
        at: new Date().toISOString(),
      }),
    );
    socket.deliver(
      JSON.stringify({
        protocol: "talos-gateway/1",
        event: "Pong",
        payload: { sequence: 7 },
        at: new Date().toISOString(),
      }),
    );
    const reply = await pending;
    expect(reply.event).toBe("Pong");
    client.disconnect();
  });

  it("rejects commands when the socket is closed", async () => {
    const client = new GatewayClient();
    await expect(client.sendCommand("Ping", {})).rejects.toMatchObject({
      code: "backend_unavailable",
    });
  });
});

describe("gateway Tauri invokes", () => {
  it("validates gateway_status responses", async () => {
    vi.stubGlobal("__TAURI_INTERNALS__", {});
    mockInvoke.mockResolvedValueOnce({
      link: "connected",
      url: "ws://127.0.0.1:8080/talos",
      missedPongs: 0,
      lastSequence: 3,
      daemonVersion: null,
    });
    const status = await getGatewayStatus();
    expect(status.link).toBe("connected");
    expect(mockInvoke).toHaveBeenCalledWith("gateway_status", undefined);
  });

  it("restarts the daemon through the shell", async () => {
    vi.stubGlobal("__TAURI_INTERNALS__", {});
    mockInvoke.mockResolvedValueOnce({
      link: "connected",
      url: "ws://127.0.0.1:8080/talos",
      missedPongs: 0,
      lastSequence: null,
      daemonVersion: null,
    });
    const status = await restartGatewayDaemon();
    expect(status.link).toBe("connected");
    expect(mockInvoke).toHaveBeenCalledWith("gateway_restart", undefined);
  });
});
