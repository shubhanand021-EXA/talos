import { SectionHeader } from "../../design-system/index.js";
import { CodeWorkspacePanel } from "./CodeWorkspacePanel.js";

/** Code page: isolated per-task workspaces with shell, tests, and git. */
export function CodePage() {
  return (
    <div className="flex flex-col gap-4">
      <SectionHeader
        title="Code"
        subtitle="Workspace-isolated file, shell, test, and git tools for the CoderAgent"
      />
      <p className="text-sm text-slate-400">
        Each coding task gets an isolated root — a git worktree when a repo is
        hinted, otherwise a temp directory. Shell execution, test runs, and
        dependency installs stay inside the active workspace root; every
        invocation is policy-gated and persisted as evidence.
      </p>
      <CodeWorkspacePanel />
    </div>
  );
}
