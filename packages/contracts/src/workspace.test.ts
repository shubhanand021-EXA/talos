import { describe, expect, it } from "vitest";
import {
  ShellReceiptSchema,
  TestResultSchema,
  WorkspaceInfoSchema,
  WorkspaceUpdatedPayloadSchema,
} from "./workspace.js";

const NOW = "2026-10-31T00:00:00.000Z";

describe("workspace contracts", () => {
  it("parses workspace info", () => {
    const info = {
      taskId: "task-1",
      root: "/tmp/talos-workspaces/task-1",
      branch: "talos/task-1",
      isGitWorktree: false,
    };
    expect(WorkspaceInfoSchema.parse(info)).toEqual(info);
  });

  it("parses shell receipts and test results", () => {
    expect(
      ShellReceiptSchema.parse({
        command: "cargo",
        exitCode: 0,
        stdoutHash: "sha256:abc",
        stderrHash: "sha256:def",
        truncated: false,
        timedOut: false,
        durationMs: 120,
        workspacePath: "/tmp/ws/task-1",
      }),
    ).toMatchObject({ command: "cargo" });
    expect(
      TestResultSchema.parse({
        framework: "cargo",
        total: 3,
        passed: 3,
        failed: 0,
        errors: 0,
        outputHash: "sha256:abc",
        durationMs: 120,
        exitCode: 0,
      }),
    ).toMatchObject({ passed: 3 });
    expect(() =>
      TestResultSchema.parse({
        framework: "cargo",
        total: -1,
        passed: 0,
        failed: 0,
        errors: 0,
        outputHash: "sha256:abc",
        durationMs: 0,
      }),
    ).toThrow();
  });

  it("parses workspace events", () => {
    expect(
      WorkspaceUpdatedPayloadSchema.parse({
        taskId: "task-1",
        path: "/tmp/ws/task-1",
        status: "created",
        checkedAt: NOW,
      }),
    ).toMatchObject({ status: "created" });
  });
});
