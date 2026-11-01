import { useEffect, useState } from "react";
import {
  DataTable,
  EmptyState,
  GlassPanel,
  KeyValueList,
  NeonButton,
  SectionHeader,
  StatusPill,
} from "../../design-system/index.js";
import {
  cleanupWorkspace,
  createWorkspace,
  getWorkspace,
  getWorkspaceTree,
  invokeTool,
  invokeToolDryRun,
  isBackendUnavailable,
  listWorkspaces,
  type ToolReceipt,
  type WorkspaceInfo,
  type WorkspaceTree,
} from "../../bridge/index.js";
import { useAppStore } from "../../stores/appStore.js";
import { useUiStore } from "../../stores/uiStore.js";

/** Render an unknown receipt field as display text (never `[object Object]`). */
function text(value: unknown, fallback = "—"): string {
  if (typeof value === "string") {
    return value;
  }
  if (typeof value === "number" || typeof value === "boolean") {
    return String(value);
  }
  return fallback;
}

/** Shell execution output (hashes, exit code, duration, truncated streams). */
export function ShellOutputView({ output }: { output: unknown }) {
  const record =
    typeof output === "object" && output !== null
      ? (output as Record<string, unknown>)
      : {};
  const stdout = typeof record.stdout === "string" ? record.stdout : "";
  const stderr = typeof record.stderr === "string" ? record.stderr : "";
  const duration =
    typeof record.durationMs === "number"
      ? `${String(record.durationMs)} ms`
      : "—";
  return (
    <div className="flex flex-col gap-2">
      <KeyValueList
        entries={[
          { key: "command", value: text(record.command) },
          { key: "exit", value: text(record.exitCode) },
          { key: "duration", value: duration },
          { key: "stdout hash", value: text(record.stdoutHash) },
          { key: "stderr hash", value: text(record.stderrHash) },
          { key: "truncated", value: text(record.truncated, "false") },
          { key: "workspace", value: text(record.workspacePath) },
        ]}
      />
      <p className="text-xs uppercase tracking-wider text-slate-500">stdout</p>
      <pre className="max-h-48 overflow-auto rounded-md border border-white/10 bg-black/40 p-2 font-mono text-xs text-slate-200">
        {stdout === "" ? "(empty)" : stdout}
      </pre>
      {stderr !== "" ? (
        <>
          <p className="text-xs uppercase tracking-wider text-slate-500">
            stderr
          </p>
          <pre className="max-h-32 overflow-auto rounded-md border border-white/10 bg-black/40 p-2 font-mono text-xs text-rose-200">
            {stderr}
          </pre>
        </>
      ) : null}
    </div>
  );
}

/** Test runner result (framework, totals, evidence hash). */
export function TestResultView({ output }: { output: unknown }) {
  const record =
    typeof output === "object" && output !== null
      ? (output as Record<string, unknown>)
      : {};
  const failed = Number(record.failed ?? 0);
  const testDuration =
    typeof record.durationMs === "number"
      ? `${String(record.durationMs)} ms`
      : "—";
  return (
    <div className="flex flex-col gap-2">
      <StatusPill tone={failed === 0 ? "ok" : "error"}>
        {failed === 0 ? "passing" : "failing"}
      </StatusPill>
      <KeyValueList
        entries={[
          { key: "framework", value: text(record.framework) },
          { key: "total", value: text(record.total) },
          { key: "passed", value: text(record.passed) },
          { key: "failed", value: text(failed) },
          { key: "errors", value: text(record.errors, "0") },
          { key: "output hash", value: text(record.outputHash) },
          { key: "duration", value: testDuration },
        ]}
      />
    </div>
  );
}

/** Unified diff viewer for `git.diff` / `fs.edit` patches. */
export function DiffView({ patch }: { patch: string }) {
  if (patch.trim() === "") {
    return <p className="text-xs text-slate-500">No changes.</p>;
  }
  return (
    <pre className="max-h-64 overflow-auto rounded-md border border-white/10 bg-black/40 p-2 font-mono text-xs leading-5">
      {patch.split("\n").map((line, index) => (
        <span
          key={index}
          className={
            line.startsWith("+") && !line.startsWith("+++")
              ? "block bg-emerald-500/10 text-emerald-200"
              : line.startsWith("-") && !line.startsWith("---")
                ? "block bg-rose-500/10 text-rose-200"
                : line.startsWith("@@")
                  ? "block text-cyan-300"
                  : "block text-slate-300"
          }
        >
          {line}
        </span>
      ))}
    </pre>
  );
}

/** File tree of a workspace root. */
export function FileTreeView({ tree }: { tree: WorkspaceTree | null }) {
  if (tree === null) {
    return (
      <EmptyState
        title="No workspace tree"
        message="Create a workspace, then load its file tree."
      />
    );
  }
  const entries = Array.isArray(tree.entries) ? tree.entries : [];
  if (entries.length === 0) {
    return <p className="text-xs text-slate-500">Empty workspace.</p>;
  }
  return (
    <ul className="flex max-h-48 flex-col gap-0.5 overflow-y-auto font-mono text-xs">
      {entries.map((entry, index) => {
        const row =
          typeof entry === "object" && entry !== null
            ? (entry as Record<string, unknown>)
            : {};
        const kind = text(row.kind, "file");
        return (
          <li key={index} className="text-slate-300">
            <span className="text-slate-500">
              {kind === "dir" ? "▸ " : "· "}
            </span>
            {text(row.name, JSON.stringify(entry))}
          </li>
        );
      })}
    </ul>
  );
}

export function CodeWorkspacePanel() {
  const liveTasks = useAppStore((state) => state.liveTasks);
  const appendFeed = useAppStore((state) => state.appendFeed);
  const selectEntity = useUiStore((state) => state.selectEntity);
  const [taskId, setTaskId] = useState("");
  const [workspace, setWorkspace] = useState<WorkspaceInfo | null>(null);
  const [tree, setTree] = useState<WorkspaceTree | null>(null);
  const [shellReceipt, setShellReceipt] = useState<ToolReceipt | null>(null);
  const [testReceipt, setTestReceipt] = useState<ToolReceipt | null>(null);
  const [diffPatch, setDiffPatch] = useState("");
  const [statusText, setStatusText] = useState("No workspace status yet.");
  const [busy, setBusy] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const activeTask =
    taskId.trim() !== "" ? taskId.trim() : (liveTasks[0]?.id ?? "");

  useEffect(() => {
    let cancelled = false;
    const refresh = async () => {
      try {
        const items = await listWorkspaces().catch(() => null);
        if (!cancelled && items !== null && items.length > 0) {
          const first = items[0];
          if (first !== undefined) {
            setWorkspace((current) => current ?? first);
          }
        }
      } catch {
        // Backend unavailable: panel stays in empty state.
      }
    };
    void refresh();
    return () => {
      cancelled = true;
    };
  }, []);

  const fail = (error: unknown, fallback: string) => {
    if (isBackendUnavailable(error)) {
      setNotice("Backend unavailable — launch the desktop shell.");
    } else {
      setNotice(error instanceof Error ? error.message : fallback);
    }
  };

  const run = async (key: string, work: () => Promise<void>) => {
    setBusy(key);
    setNotice(null);
    try {
      await work();
    } catch (error) {
      fail(error, `${key} failed`);
    } finally {
      setBusy(null);
    }
  };

  const handleCreate = () =>
    run("create", async () => {
      const info = await createWorkspace(activeTask);
      setWorkspace(info);
      appendFeed({
        id: `ws-create-${Date.now()}`,
        at: new Date().toISOString(),
        actor: "workspace",
        message: `Workspace created for ${info.taskId}: ${info.root}`,
        tone: "success",
        taskId: info.taskId,
      });
    });

  const handleRefresh = () =>
    run("refresh", async () => {
      const info = await getWorkspace(activeTask);
      setWorkspace(info);
      const loaded = await getWorkspaceTree(activeTask, 2);
      setTree(loaded);
    });

  const handleCleanup = () =>
    run("cleanup", async () => {
      await cleanupWorkspace(activeTask);
      setWorkspace(null);
      setTree(null);
    });

  const handleShell = (dryRun: boolean) =>
    run(dryRun ? "shell-dry" : "shell", async () => {
      const receipt = dryRun
        ? await invokeToolDryRun("process.exec", activeTask, {
            command: "git",
            args: ["--version"],
            timeoutMs: 60000,
          })
        : await invokeTool("process.exec", activeTask, {
            command: "git",
            args: ["--version"],
            timeoutMs: 60000,
          });
      setShellReceipt(receipt);
      selectEntity({
        kind: "system",
        id: receipt.id,
        label: `Shell ${receipt.status}: git --version`,
        detail: receipt.policyReason,
      });
    });

  const handleTests = () =>
    run("tests", async () => {
      const receipt = await invokeTool("code.run_tests", activeTask, {});
      setTestReceipt(receipt);
    });

  const handleDiff = () =>
    run("diff", async () => {
      const receipt = await invokeTool("git.diff", activeTask, {
        statOnly: false,
        maxChars: 8000,
      });
      const output = receipt.output as Record<string, unknown>;
      setDiffPatch(typeof output.patch === "string" ? output.patch : "");
      const status = await invokeTool("git.status", activeTask, {});
      const statusOut = status.output as Record<string, unknown>;
      const branch =
        typeof statusOut.branch === "string" ? statusOut.branch : "—";
      const clean =
        typeof statusOut.clean === "boolean" ? text(statusOut.clean) : "?";
      setStatusText(`branch=${branch} clean=${clean}`);
    });

  return (
    <div className="flex flex-col gap-4">
      <SectionHeader
        title="Code workspace"
        subtitle={
          workspace === null
            ? "No workspace selected — isolate a task root first"
            : `${workspace.taskId} → ${workspace.root}${workspace.isGitWorktree ? " (worktree)" : ""}`
        }
      />
      {notice !== null ? (
        <p role="status" className="text-xs text-amber-300">
          {notice}
        </p>
      ) : null}
      <div className="flex flex-wrap gap-2">
        <input
          aria-label="Workspace task id"
          placeholder={liveTasks[0]?.id ?? "task id"}
          value={taskId}
          onChange={(event) => {
            setTaskId(event.target.value);
          }}
          className="min-w-52 flex-1 rounded-md border border-white/10 bg-black/30 px-2 py-1 font-mono text-xs"
        />
        <NeonButton
          onClick={() => void handleCreate()}
          loading={busy === "create"}
        >
          Create workspace
        </NeonButton>
        <NeonButton
          onClick={() => void handleRefresh()}
          loading={busy === "refresh"}
        >
          Load tree
        </NeonButton>
        <NeonButton
          onClick={() => void handleCleanup()}
          loading={busy === "cleanup"}
        >
          Cleanup
        </NeonButton>
      </div>
      <GlassPanel label="Active workspace">
        {workspace === null ? (
          <EmptyState
            title="No active workspace"
            message="Enter a task id and create its isolated root (git worktree or temp dir)."
          />
        ) : (
          <KeyValueList
            entries={[
              { key: "task", value: workspace.taskId },
              { key: "root", value: workspace.root },
              { key: "branch", value: workspace.branch },
              { key: "worktree", value: String(workspace.isGitWorktree) },
              { key: "status", value: statusText },
            ]}
          />
        )}
      </GlassPanel>
      <div className="grid grid-cols-1 gap-4 2xl:grid-cols-2">
        <GlassPanel label="File tree">
          <div className="mb-2 flex items-center justify-between gap-2">
            <NeonButton
              onClick={() => void handleDiff()}
              loading={busy === "diff"}
            >
              Refresh status + diff
            </NeonButton>
          </div>
          <FileTreeView tree={tree} />
        </GlassPanel>
        <GlassPanel label="Diff viewer">
          <DiffView patch={diffPatch} />
        </GlassPanel>
      </div>
      <div className="grid grid-cols-1 gap-4 2xl:grid-cols-2">
        <GlassPanel label="Shell execution">
          <div className="mb-2 flex gap-2">
            <NeonButton
              onClick={() => void handleShell(true)}
              loading={busy === "shell-dry"}
            >
              Dry run
            </NeonButton>
            <NeonButton
              onClick={() => void handleShell(false)}
              loading={busy === "shell"}
            >
              Run git --version
            </NeonButton>
          </div>
          {shellReceipt === null ? (
            <EmptyState
              title="No shell runs yet"
              message="Dry-run plans without spawning; live runs execute inside the workspace root."
            />
          ) : (
            <ShellOutputView output={shellReceipt.output} />
          )}
        </GlassPanel>
        <GlassPanel label="Test results">
          <div className="mb-2 flex gap-2">
            <NeonButton
              onClick={() => void handleTests()}
              loading={busy === "tests"}
            >
              Run tests
            </NeonButton>
          </div>
          {testReceipt === null ? (
            <EmptyState
              title="No test runs yet"
              message="Auto-detects cargo / pnpm / npm / pytest from workspace manifests."
            />
          ) : (
            <TestResultView output={testReceipt.output} />
          )}
        </GlassPanel>
      </div>
      <GlassPanel label="Workspaces">
        <WorkspaceList onSelect={setWorkspace} />
      </GlassPanel>
    </div>
  );
}

function WorkspaceList({
  onSelect,
}: {
  onSelect: (info: WorkspaceInfo) => void;
}) {
  const [items, setItems] = useState<WorkspaceInfo[]>([]);
  const [loading, setLoading] = useState(false);
  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      setLoading(true);
      try {
        const listed = await listWorkspaces();
        if (!cancelled) {
          setItems(listed);
        }
      } catch {
        if (!cancelled) {
          setItems([]);
        }
      } finally {
        if (!cancelled) {
          setLoading(false);
        }
      }
    };
    void load();
    return () => {
      cancelled = true;
    };
  }, []);
  return (
    <DataTable<WorkspaceInfo>
      caption={loading ? "Loading workspaces…" : `${items.length} workspaces`}
      rows={items}
      keyOf={(row) => row.taskId}
      onRowSelect={(row) => {
        onSelect(row);
      }}
      columns={[
        { header: "Task", render: (row) => row.taskId },
        {
          header: "Root",
          render: (row) => (
            <span className="font-mono text-xs">{row.root}</span>
          ),
        },
        { header: "Branch", render: (row) => row.branch },
        {
          header: "Worktree",
          render: (row) => (
            <StatusPill tone={row.isGitWorktree ? "ok" : "neutral"}>
              {row.isGitWorktree ? "worktree" : "isolated"}
            </StatusPill>
          ),
        },
      ]}
    />
  );
}
