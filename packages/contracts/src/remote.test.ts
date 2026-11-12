import { describe, expect, it } from "vitest";
import {
  ExecutionTargetTypeSchema,
  RemoteRunnerConfigSchema,
  RunnerStatusViewSchema,
  SetTaskRunnerSchema,
} from "./remote.js";

const SSH_RUNNER = {
  id: "gpu-box",
  kind: "ssh",
  host: "gpu.local",
  user: "talos",
  keyPath: "~/.ssh/talos_ed25519",
  port: 2222,
  remoteDir: "~/.talos-runs",
  enabled: true,
};

describe("remote runner contracts", () => {
  it("validates the execution target switch", () => {
    expect(ExecutionTargetTypeSchema.parse("local")).toBe("local");
    expect(ExecutionTargetTypeSchema.parse("remote")).toBe("remote");
    expect(() => ExecutionTargetTypeSchema.parse("cloud")).toThrow();
  });

  it("validates an SSH runner definition", () => {
    expect(RemoteRunnerConfigSchema.safeParse(SSH_RUNNER).success).toBe(true);
  });

  it("validates a Docker runner definition", () => {
    expect(
      RemoteRunnerConfigSchema.safeParse({
        id: "docker-t4",
        kind: "docker",
        dockerImage: "talos-runner:1",
        enabled: true,
      }).success,
    ).toBe(true);
  });

  it("rejects bad ids and unknown kinds", () => {
    expect(
      RemoteRunnerConfigSchema.safeParse({ ...SSH_RUNNER, id: "Bad ID!" })
        .success,
    ).toBe(false);
    expect(
      RemoteRunnerConfigSchema.safeParse({ ...SSH_RUNNER, kind: "vm" }).success,
    ).toBe(false);
  });

  it("validates status rows and task assignment", () => {
    expect(
      RunnerStatusViewSchema.safeParse({
        id: "gpu-box",
        kind: "ssh",
        enabled: true,
        lastTestOk: true,
        lastTestMessage: "reachable",
      }).success,
    ).toBe(true);
    expect(
      SetTaskRunnerSchema.safeParse({ taskId: "task-1", runnerId: "gpu-box" })
        .success,
    ).toBe(true);
    expect(
      SetTaskRunnerSchema.safeParse({ taskId: "task-1", runnerId: null })
        .success,
    ).toBe(true);
  });
});
