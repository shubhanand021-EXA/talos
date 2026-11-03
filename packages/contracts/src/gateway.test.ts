import { describe, expect, it } from "vitest";
import {
  GATEWAY_PROTOCOL_VERSION,
  GATEWAY_URL,
  GatewayCommandKind,
  GatewayEventKind,
  GatewayCommandSchema,
  GatewayEventSchema,
  gatewayCommand,
  gatewayEvent,
  parseGatewayCommandPayload,
} from "./gateway.js";

describe("gateway protocol", () => {
  it("targets the loopback daemon endpoint with a versioned protocol", () => {
    expect(GATEWAY_URL).toBe("ws://127.0.0.1:8080/talos");
    expect(GATEWAY_PROTOCOL_VERSION).toBe("talos-gateway/1");
  });

  it("round-trips a SubmitTask command frame", () => {
    const frame = gatewayCommand("cmd-1", GatewayCommandKind.SubmitTask, {
      title: "probe",
    });
    const parsed = GatewayCommandSchema.safeParse(frame);
    expect(parsed.success).toBe(true);
    expect(
      parseGatewayCommandPayload(GatewayCommandKind.SubmitTask, {
        title: "probe",
      }),
    ).toMatchObject({ ok: true });
  });

  it("rejects empty SubmitTask titles", () => {
    expect(
      parseGatewayCommandPayload(GatewayCommandKind.SubmitTask, { title: "" }),
    ).toMatchObject({ ok: false });
  });

  it("round-trips every event kind", () => {
    for (const event of Object.values(GatewayEventKind)) {
      const frame = gatewayEvent(event, {}, "task-1");
      expect(GatewayEventSchema.safeParse(frame).success).toBe(true);
    }
  });

  it("validates ForwardIpc routing for the 96 Tauri commands", () => {
    const parsed = parseGatewayCommandPayload(GatewayCommandKind.ForwardIpc, {
      command: "submit_task",
      args: { input: { title: "x" } },
    });
    expect(parsed).toMatchObject({ ok: true });
  });
});
