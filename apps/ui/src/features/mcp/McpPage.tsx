import { useCallback, useEffect, useState } from "react";
import type {
  McpServerInfo,
  McpTool,
  PermissionReview,
  PluginInfo,
} from "@talos/contracts";
import {
  DataTable,
  EmptyState,
  GlassPanel,
  KeyValueList,
  Modal,
  NeonButton,
  SectionHeader,
  StatusPill,
  type PillTone,
} from "../../design-system/index.js";
import {
  addMcpServer,
  approvePlugin,
  callMcpTool,
  isBackendUnavailable,
  listMcpServers,
  listMcpTools,
  listPlugins,
  parseInstructionFile,
  removeMcpServer,
} from "../../bridge/index.js";

const STATUS_TONE: Record<string, PillTone> = {
  connected: "ok",
  disconnected: "neutral",
  error: "error",
};

const TRUST_TONE: Record<string, PillTone> = {
  trusted: "ok",
  pending: "warn",
  rejected: "error",
};

export function McpPage() {
  const [servers, setServers] = useState<McpServerInfo[]>([]);
  const [plugins, setPlugins] = useState<PluginInfo[]>([]);
  const [tools, setTools] = useState<McpTool[]>([]);
  const [toolsFor, setToolsFor] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const [newName, setNewName] = useState("fs-helper");
  const [newCommand, setNewCommand] = useState("npx");
  const [newCapabilities, setNewCapabilities] = useState("tools");
  const [review, setReview] = useState<PermissionReview | null>(null);
  const [reviewFor, setReviewFor] = useState<string | null>(null);

  const [instructionPath, setInstructionPath] = useState("AGENTS.md");
  const [instructionSummary, setInstructionSummary] = useState<string | null>(
    null,
  );

  const [callServer, setCallServer] = useState("");
  const [callTool, setCallTool] = useState("");
  const [callArgs, setCallArgs] = useState("{}");
  const [callResult, setCallResult] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const [serverList, pluginList] = await Promise.all([
        listMcpServers(),
        listPlugins(),
      ]);
      setServers(serverList);
      setPlugins(pluginList);
      setNotice(null);
    } catch (error) {
      setNotice(
        isBackendUnavailable(error)
          ? "Backend unavailable — launch the desktop shell for live MCP state."
          : error instanceof Error
            ? error.message
            : "MCP refresh failed",
      );
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    // Initial load only; Refresh button reuses the same fetcher.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    void refresh();
  }, [refresh]);

  const handleAdd = async () => {
    setNotice(null);
    try {
      const capabilities = newCapabilities
        .split(",")
        .map((entry) => entry.trim())
        .filter((entry) => entry !== "");
      const { server, review: permissionReview } = await addMcpServer(newName, {
        command: newCommand,
        args: [],
        env: {},
        transport: "stdio",
        capabilities,
      });
      setServers((current) => [...current, server]);
      if (permissionReview !== null) {
        setReview(permissionReview);
        setReviewFor(server.name);
      }
    } catch (error) {
      setNotice(error instanceof Error ? error.message : "Add server failed");
    }
  };

  const handleRemove = async (name: string) => {
    setNotice(null);
    try {
      await removeMcpServer(name);
      setServers((current) => current.filter((server) => server.name !== name));
    } catch (error) {
      setNotice(
        error instanceof Error ? error.message : "Remove server failed",
      );
    }
  };

  const handleDiscover = async (name: string) => {
    setNotice(null);
    try {
      const discovered = await listMcpTools(name);
      setTools(discovered);
      setToolsFor(name);
      setCallServer(name);
      await refresh();
    } catch (error) {
      setNotice(error instanceof Error ? error.message : "Discovery failed");
    }
  };

  const handleCall = async () => {
    setNotice(null);
    try {
      const args = JSON.parse(callArgs) as unknown;
      const receipt = await callMcpTool(
        callServer,
        callTool,
        "task-local-1",
        args,
      );
      setCallResult(
        `${receipt.decision}: ${receipt.reason} (evidence ${receipt.evidenceId ?? "—"})`,
      );
    } catch (error) {
      if (error instanceof SyntaxError) {
        setNotice("Call args must be valid JSON.");
      } else {
        setNotice(error instanceof Error ? error.message : "Call failed");
      }
    }
  };

  const handleParse = async () => {
    setNotice(null);
    try {
      const view = await parseInstructionFile(instructionPath);
      setInstructionSummary(
        `${view.kind}: ${JSON.stringify(view.summary).slice(0, 2000)}`,
      );
    } catch (error) {
      setNotice(error instanceof Error ? error.message : "Parse failed");
    }
  };

  const handleApprove = async (name: string, approved: boolean) => {
    setNotice(null);
    try {
      await approvePlugin(name, approved);
      setReview(null);
      setReviewFor(null);
      await refresh();
    } catch (error) {
      setNotice(
        error instanceof Error ? error.message : "Approval update failed",
      );
    }
  };

  return (
    <div className="flex flex-col gap-4">
      <SectionHeader
        title="MCP & Plugins"
        subtitle={
          loading
            ? "Loading…"
            : `${servers.length} servers · ${plugins.length} plugins · policy-gated`
        }
        action={
          <NeonButton
            onClick={() => {
              void refresh();
            }}
            loading={loading}
          >
            Refresh
          </NeonButton>
        }
      />
      {notice !== null ? (
        <p role="status" className="text-xs text-amber-300">
          {notice}
        </p>
      ) : null}

      <GlassPanel label="MCP servers">
        {servers.length === 0 ? (
          <EmptyState
            title="No MCP servers"
            message="Add a stdio or SSE server below. Every tool call is policy-gated."
          />
        ) : (
          <DataTable<McpServerInfo>
            caption="MCP servers"
            rows={servers}
            keyOf={(row) => row.name}
            columns={[
              { header: "Server", render: (row) => row.name },
              { header: "Transport", render: (row) => row.transport },
              {
                header: "Status",
                render: (row) => (
                  <StatusPill tone={STATUS_TONE[row.status] ?? "neutral"}>
                    {row.status}
                  </StatusPill>
                ),
              },
              {
                header: "Trust",
                render: (row) => (
                  <StatusPill tone={TRUST_TONE[row.trust] ?? "neutral"}>
                    {row.trust}
                  </StatusPill>
                ),
              },
              {
                header: "Capabilities",
                render: (row) => row.capabilities.join(", "),
              },
              {
                header: "Actions",
                render: (row) => (
                  <div className="flex gap-2">
                    <NeonButton
                      onClick={() => {
                        void handleDiscover(row.name);
                      }}
                    >
                      Discover
                    </NeonButton>
                    <NeonButton
                      onClick={() => {
                        void handleRemove(row.name);
                      }}
                    >
                      Remove
                    </NeonButton>
                  </div>
                ),
              },
            ]}
          />
        )}
      </GlassPanel>

      <GlassPanel label="Add MCP server">
        <div className="grid grid-cols-1 gap-2 md:grid-cols-3">
          <label className="flex flex-col gap-1 text-xs text-slate-400">
            Name
            <input
              value={newName}
              onChange={(event) => {
                setNewName(event.target.value);
              }}
              className="rounded-md border border-white/10 bg-black/30 px-2 py-1 text-sm text-slate-100"
            />
          </label>
          <label className="flex flex-col gap-1 text-xs text-slate-400">
            Command (stdio)
            <input
              value={newCommand}
              onChange={(event) => {
                setNewCommand(event.target.value);
              }}
              className="rounded-md border border-white/10 bg-black/30 px-2 py-1 text-sm text-slate-100"
            />
          </label>
          <label className="flex flex-col gap-1 text-xs text-slate-400">
            Capabilities (comma-separated)
            <input
              value={newCapabilities}
              onChange={(event) => {
                setNewCapabilities(event.target.value);
              }}
              className="rounded-md border border-white/10 bg-black/30 px-2 py-1 text-sm text-slate-100"
            />
          </label>
        </div>
        <div className="mt-2">
          <NeonButton
            onClick={() => {
              void handleAdd();
            }}
          >
            Add server
          </NeonButton>
        </div>
      </GlassPanel>

      <GlassPanel label="Tool discovery">
        {toolsFor === null ? (
          <EmptyState
            title="No tools discovered"
            message="Pick Discover on a server to list its tools."
          />
        ) : (
          <div className="flex flex-col gap-2">
            <p className="text-xs text-slate-400">
              {tools.length} tools from {toolsFor}
            </p>
            <KeyValueList
              entries={tools.map((tool) => ({
                key: tool.name,
                value: tool.description,
              }))}
            />
          </div>
        )}
        <div className="mt-2 grid grid-cols-1 gap-2 md:grid-cols-3">
          <label className="flex flex-col gap-1 text-xs text-slate-400">
            Server
            <input
              value={callServer}
              onChange={(event) => {
                setCallServer(event.target.value);
              }}
              className="rounded-md border border-white/10 bg-black/30 px-2 py-1 text-sm text-slate-100"
            />
          </label>
          <label className="flex flex-col gap-1 text-xs text-slate-400">
            Tool
            <input
              value={callTool}
              onChange={(event) => {
                setCallTool(event.target.value);
              }}
              className="rounded-md border border-white/10 bg-black/30 px-2 py-1 text-sm text-slate-100"
            />
          </label>
          <label className="flex flex-col gap-1 text-xs text-slate-400">
            Args (JSON)
            <input
              value={callArgs}
              onChange={(event) => {
                setCallArgs(event.target.value);
              }}
              className="rounded-md border border-white/10 bg-black/30 px-2 py-1 font-mono text-sm text-slate-100"
            />
          </label>
        </div>
        <div className="mt-2 flex items-center gap-2">
          <NeonButton
            onClick={() => {
              void handleCall();
            }}
          >
            Call tool
          </NeonButton>
          {callResult !== null ? (
            <p role="status" className="text-xs text-slate-300">
              {callResult}
            </p>
          ) : null}
        </div>
      </GlassPanel>

      <GlassPanel label="Instruction files">
        <div className="flex gap-2">
          <input
            aria-label="Instruction file path"
            value={instructionPath}
            onChange={(event) => {
              setInstructionPath(event.target.value);
            }}
            className="flex-1 rounded-md border border-white/10 bg-black/30 px-2 py-1 text-sm text-slate-100"
          />
          <NeonButton
            onClick={() => {
              void handleParse();
            }}
          >
            Parse file
          </NeonButton>
        </div>
        {instructionSummary !== null ? (
          <pre className="mt-2 max-h-64 overflow-auto rounded-md border border-white/10 bg-black/40 p-2 font-mono text-xs text-slate-200">
            {instructionSummary}
          </pre>
        ) : (
          <p className="mt-2 text-xs text-slate-500">
            Parses AGENTS.md, CLAUDE.md, SKILL.md, and HEARTBEAT.md. Instruction
            content is safety-critical and never compressed.
          </p>
        )}
      </GlassPanel>

      <GlassPanel label="Plugins & skills">
        {plugins.length === 0 ? (
          <EmptyState
            title="No plugins yet"
            message="MCP servers with manifests and ledger skills appear here with trust status."
          />
        ) : (
          <DataTable<PluginInfo>
            caption="Plugins and skills"
            rows={plugins}
            keyOf={(row) => `${row.kind}:${row.name}`}
            columns={[
              { header: "Plugin", render: (row) => row.name },
              { header: "Kind", render: (row) => row.kind },
              {
                header: "Trust",
                render: (row) => (
                  <StatusPill tone={TRUST_TONE[row.trust] ?? "neutral"}>
                    {row.trust}
                  </StatusPill>
                ),
              },
              {
                header: "Permissions",
                render: (row) => row.permissions.join(", ") || "—",
              },
              {
                header: "Actions",
                render: (row) =>
                  row.trust === "pending" ? (
                    <div className="flex gap-2">
                      <NeonButton
                        onClick={() => {
                          void handleApprove(row.name, true);
                        }}
                      >
                        Approve
                      </NeonButton>
                      <NeonButton
                        onClick={() => {
                          void handleApprove(row.name, false);
                        }}
                      >
                        Reject
                      </NeonButton>
                    </div>
                  ) : (
                    <span className="text-xs text-slate-500">
                      {row.unsigned ? "unsigned" : "signed"}
                    </span>
                  ),
              },
            ]}
          />
        )}
      </GlassPanel>

      {review !== null ? (
        <Modal
          title={`Permission review: ${review.plugin}`}
          onClose={() => {
            setReview(null);
            setReviewFor(null);
          }}
        >
          <div className="flex flex-col gap-2">
            <p className="text-xs text-slate-400">
              {review.unsigned
                ? "Unsigned plugin — enabling requires explicit approval."
                : "Signed plugin — signature verified."}
            </p>
            <KeyValueList
              entries={[
                {
                  key: "requested permissions",
                  value: review.requested.join(", ") || "—",
                },
                {
                  key: "capabilities",
                  value: review.capabilities.join(", ") || "—",
                },
              ]}
            />
            <div className="flex gap-2">
              <NeonButton
                onClick={() => {
                  if (reviewFor !== null) {
                    void handleApprove(reviewFor, true);
                  }
                }}
              >
                Approve
              </NeonButton>
              <NeonButton
                onClick={() => {
                  if (reviewFor !== null) {
                    void handleApprove(reviewFor, false);
                  }
                }}
              >
                Reject
              </NeonButton>
            </div>
          </div>
        </Modal>
      ) : null}
    </div>
  );
}
