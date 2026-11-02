import { describe, expect, it } from "vitest";
import {
  SandboxBackendSchema,
  SandboxConfigSchema,
  SandboxEnforcedPayloadSchema,
  SandboxProfileForTaskSchema,
  SandboxProfileSchema,
  SandboxReceiptSchema,
  SandboxStatusSchema,
  SandboxViolationPayloadSchema,
} from "./sandbox.js";

function config(overrides: Record<string, unknown> = {}) {
  return {
    profile: "workspace_write",
    workspaceRoot: "/tmp/talos-ws",
    networkAllowed: false,
    allowedHosts: [],
    timeoutMs: 60_000,
    allowedCommands: ["cargo", "pytest"],
    ...overrides,
  };
}

describe("sandbox contracts", () => {
  it("parses profiles and rejects unknown ones", () => {
    expect(SandboxProfileSchema.parse("read_only")).toBe("read_only");
    expect(SandboxProfileSchema.parse("workspace_write")).toBe(
      "workspace_write",
    );
    expect(SandboxProfileSchema.parse("danger_full_access")).toBe(
      "danger_full_access",
    );
    expect(() => SandboxProfileSchema.parse("unconfined")).toThrow();
  });

  it("parses sandbox configs with resource limits", () => {
    const parsed = SandboxConfigSchema.parse(
      config({
        profile: "read_only",
        cpuLimitPercent: 50,
        memoryLimitMb: 512,
        allowedHosts: ["proxy.internal:8080"],
      }),
    );
    expect(parsed.cpuLimitPercent).toBe(50);
    expect(parsed.networkAllowed).toBe(false);
    expect(() =>
      SandboxConfigSchema.parse(config({ profile: "root" })),
    ).toThrow();
    expect(() =>
      SandboxConfigSchema.parse(config({ cpuLimitPercent: 101 })),
    ).toThrow();
    expect(() =>
      SandboxConfigSchema.parse(config({ memoryLimitMb: 8 })),
    ).toThrow();
  });

  it("parses sandbox receipts and statuses", () => {
    const receipt = {
      profile: "workspace_write",
      enforced: true,
      enforcementMethod: "bubblewrap+prlimit",
      networkPolicy: "no-network (unshare-net)",
      violations: [],
    };
    expect(SandboxReceiptSchema.parse(receipt)).toEqual(receipt);
    const status = {
      platform: "linux",
      failClosed: true,
      backends: [
        {
          name: "bubblewrap",
          available: true,
          enforcementMethod: "bubblewrap+prlimit",
          detail: "bwrap 0.8.0",
        },
      ],
      checkedAt: "2026-11-02T00:00:00.000Z",
    };
    expect(SandboxStatusSchema.parse(status)).toEqual(status);
    expect(SandboxBackendSchema.parse(status.backends[0])).toMatchObject({
      available: true,
    });
  });

  it("parses profile-for-task and event payloads", () => {
    expect(
      SandboxProfileForTaskSchema.parse({
        taskId: "task-1",
        profile: "read_only",
        enforced: true,
        enforcementMethod: "deny-exec(read-only)",
        reason: "read-only profile forbids process execution",
      }),
    ).toMatchObject({ profile: "read_only" });
    expect(
      SandboxEnforcedPayloadSchema.parse({
        taskId: "task-1",
        toolId: "process.exec",
        profile: "workspace_write",
        enforcementMethod: "bubblewrap+prlimit",
        checkedAt: "2026-11-02T00:00:00.000Z",
      }),
    ).toMatchObject({ toolId: "process.exec" });
    expect(
      SandboxViolationPayloadSchema.parse({
        taskId: "task-1",
        code: "sandbox_unavailable",
        detail: "no OS sandbox backend available; execution denied",
        checkedAt: "2026-11-02T00:00:00.000Z",
      }),
    ).toMatchObject({ code: "sandbox_unavailable" });
  });
});
