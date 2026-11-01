import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import {
  DiffView,
  FileTreeView,
  ShellOutputView,
  TestResultView,
} from "./CodeWorkspacePanel.js";

describe("code workspace panel", () => {
  it("renders shell output with hashes and streams", () => {
    render(
      <ShellOutputView
        output={{
          command: "git",
          exitCode: 0,
          stdout: "git version 2.44.0",
          stderr: "",
          stdoutHash: "sha256:abc",
          stderrHash: "sha256:def",
          truncated: false,
          timedOut: false,
          durationMs: 42,
          workspacePath: "/tmp/ws/task-1",
        }}
      />,
    );
    expect(screen.getByText("git version 2.44.0")).toBeInTheDocument();
    expect(screen.getByText("sha256:abc")).toBeInTheDocument();
    expect(screen.getByText("42 ms")).toBeInTheDocument();
  });

  it("renders empty shell streams honestly", () => {
    render(
      <ShellOutputView output={{ command: "ls", stdout: "", stderr: "" }} />,
    );
    expect(screen.getByText("(empty)")).toBeInTheDocument();
  });

  it("renders passing and failing test results", () => {
    const { rerender } = render(
      <TestResultView
        output={{
          framework: "cargo",
          total: 3,
          passed: 3,
          failed: 0,
          errors: 0,
          outputHash: "sha256:abc",
          durationMs: 120,
          exitCode: 0,
        }}
      />,
    );
    expect(screen.getByText("passing")).toBeInTheDocument();
    rerender(
      <TestResultView
        output={{
          framework: "pytest",
          total: 4,
          passed: 3,
          failed: 1,
          errors: 0,
          outputHash: "sha256:def",
          durationMs: 90,
          exitCode: 1,
        }}
      />,
    );
    expect(screen.getByText("failing")).toBeInTheDocument();
  });

  it("renders diffs with added and removed lines", () => {
    render(<DiffView patch={`@@ -1,2 +1,2 @@\n-old\n+new\n context`} />);
    expect(screen.getByText("-old")).toBeInTheDocument();
    expect(screen.getByText("+new")).toBeInTheDocument();
  });

  it("renders empty diffs and empty trees", () => {
    render(<DiffView patch="" />);
    expect(screen.getByText("No changes.")).toBeInTheDocument();
    render(<FileTreeView tree={null} />);
    expect(screen.getByText("No workspace tree")).toBeInTheDocument();
    render(
      <FileTreeView tree={{ path: ".", entries: [], truncated: false }} />,
    );
    expect(screen.getByText("Empty workspace.")).toBeInTheDocument();
  });

  it("renders file tree entries", () => {
    render(
      <FileTreeView
        tree={{
          path: ".",
          entries: [
            { name: "src", kind: "dir" },
            { name: "main.rs", kind: "file" },
          ],
          truncated: false,
        }}
      />,
    );
    expect(screen.getByText("src")).toBeInTheDocument();
    expect(screen.getByText("main.rs")).toBeInTheDocument();
  });
});
