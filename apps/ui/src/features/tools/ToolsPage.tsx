import { useEffect, useState } from "react";
import type { ToolManifest, ToolReceipt } from "@talos/contracts";
import {
  DataTable,
  EmptyState,
  GlassPanel,
  KeyValueList,
  NeonButton,
  SectionHeader,
  StatusPill,
  type PillTone,
} from "../../design-system/index.js";
import {
  describeTool,
  invokeTool,
  invokeToolDryRun,
  isBackendUnavailable,
  listTools,
} from "../../bridge/index.js";
import { useUiStore } from "../../stores/uiStore.js";
import { ToolRagPanel } from "./ToolRagPanel.js";

const RISK_TONE: Record<string, PillTone> = {
  Low: "ok",
  low: "ok",
  Medium: "warn",
  medium: "warn",
  High: "error",
  high: "error",
  Irreversible: "error",
  irreversible: "error",
};

const STATUS_TONE: Record<string, PillTone> = {
  success: "ok",
  failed: "error",
  denied: "warn",
  disabled: "neutral",
};

const PARAM_TEMPLATES: Record<string, string> = {
  "fs.read": `{\n  "path": "README.md"\n}`,
  "fs.list": `{\n  "path": ".",\n  "depth": 1\n}`,
  "fs.search": `{\n  "query": "Talos",\n  "path": ".",\n  "maxMatches": 20\n}`,
  "fs.write": `{\n  "path": "notes/scratch.txt",\n  "content": "hello from Talos",\n  "createParents": true\n}`,
  "process.exec": `{\n  "command": "echo",\n  "args": ["hello"],\n  "timeoutMs": 5000\n}`,
};

export function ToolsPage() {
  const selectEntity = useUiStore((state) => state.selectEntity);
  const [catalog, setCatalog] = useState<ToolManifest[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [manifest, setManifest] = useState<ToolManifest | null>(null);
  const [taskId, setTaskId] = useState("task-local-1");
  const [paramsText, setParamsText] = useState(
    PARAM_TEMPLATES["fs.read"] ?? "{}",
  );
  const [receipt, setReceipt] = useState<ToolReceipt | null>(null);
  const [running, setRunning] = useState(false);
  const [loading, setLoading] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    const refresh = async () => {
      setLoading(true);
      try {
        const tools = await listTools();
        if (!cancelled) {
          setCatalog(tools);
          setNotice(null);
          const first = tools[0]?.id ?? null;
          setSelectedId((current) => current ?? first);
        }
      } catch (error) {
        if (!cancelled) {
          setNotice(
            isBackendUnavailable(error)
              ? "Backend unavailable — launch the desktop shell for the live catalog."
              : error instanceof Error
                ? error.message
                : "Catalog load failed",
          );
        }
      } finally {
        if (!cancelled) {
          setLoading(false);
        }
      }
    };
    void refresh();
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      if (selectedId === null) {
        setManifest(null);
        return;
      }
      try {
        const detail = await describeTool(selectedId);
        if (!cancelled) {
          setManifest(detail);
          setParamsText(PARAM_TEMPLATES[detail.id] ?? paramsText);
        }
      } catch {
        if (!cancelled) {
          setManifest(catalog.find((tool) => tool.id === selectedId) ?? null);
        }
      }
    };
    void load();
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedId]);

  const parseParams = (): unknown => {
    return JSON.parse(paramsText) as unknown;
  };

  const runInvocation = async (dryRun: boolean) => {
    if (selectedId === null) {
      setNotice("Select a tool first.");
      return;
    }
    setRunning(true);
    setNotice(null);
    try {
      const params = parseParams();
      const result = dryRun
        ? await invokeToolDryRun(selectedId, taskId, params)
        : await invokeTool(selectedId, taskId, params);
      setReceipt(result);
      selectEntity({
        kind: "system",
        id: result.id,
        label: `Tool ${result.status}: ${selectedId}`,
        detail: result.policyReason,
      });
    } catch (error) {
      if (error instanceof SyntaxError) {
        setNotice("Params must be valid JSON.");
      } else if (isBackendUnavailable(error)) {
        setNotice("Backend unavailable — launch the desktop shell.");
      } else {
        setNotice(error instanceof Error ? error.message : "Invocation failed");
      }
    } finally {
      setRunning(false);
    }
  };

  const selected =
    manifest ?? catalog.find((tool) => tool.id === selectedId) ?? null;

  return (
    <div className="flex flex-col gap-4">
      <SectionHeader
        title="Tools"
        subtitle={
          loading
            ? "Loading catalog…"
            : `${catalog.length} registered · policy-gated · evidence-backed`
        }
        action={
          <div className="flex gap-2">
            <NeonButton
              onClick={() => {
                void runInvocation(true);
              }}
              loading={running}
            >
              Dry run
            </NeonButton>
            <NeonButton
              onClick={() => {
                void runInvocation(false);
              }}
              loading={running}
            >
              Invoke live
            </NeonButton>
          </div>
        }
      />
      {notice !== null ? (
        <p role="status" className="text-xs text-amber-300">
          {notice}
        </p>
      ) : null}
      <GlassPanel label="Tool catalog">
        {catalog.length === 0 ? (
          <EmptyState
            title="No tools yet"
            message="Launch the desktop shell to load the live registry."
          />
        ) : (
          <DataTable<ToolManifest>
            caption="Tool catalog"
            rows={catalog}
            keyOf={(row) => row.id}
            onRowSelect={(row) => {
              setSelectedId(row.id);
              selectEntity({
                kind: "system",
                id: row.id,
                label: row.name,
                detail: row.description,
              });
            }}
            columns={[
              { header: "Tool", render: (row) => row.name },
              {
                header: "Risk",
                render: (row) => (
                  <StatusPill tone={RISK_TONE[row.riskLevel] ?? "neutral"}>
                    {row.riskLevel}
                  </StatusPill>
                ),
              },
              {
                header: "Reversible",
                render: (row) => (row.reversible ? "yes" : "no"),
              },
              {
                header: "Enabled",
                render: (row) => (
                  <StatusPill tone={row.enabledByDefault ? "ok" : "neutral"}>
                    {row.enabledByDefault ? "by default" : "opt-in"}
                  </StatusPill>
                ),
              },
              {
                header: "Capabilities",
                render: (row) => row.capabilities.join(", "),
              },
            ]}
          />
        )}
      </GlassPanel>
      <div className="grid grid-cols-1 gap-4 2xl:grid-cols-2">
        <GlassPanel label="Manifest">
          {selected === null ? (
            <EmptyState title="No tool selected" message="Pick a row above." />
          ) : (
            <div className="flex flex-col gap-2">
              <SectionHeader
                title={selected.name}
                subtitle={selected.description}
              />
              <KeyValueList
                entries={[
                  { key: "version", value: selected.version },
                  { key: "risk", value: selected.riskLevel },
                  {
                    key: "requires evidence",
                    value: String(selected.requiresEvidence),
                  },
                  {
                    key: "requires compensation",
                    value: String(selected.requiresCompensation),
                  },
                ]}
              />
            </div>
          )}
        </GlassPanel>
        <GlassPanel label="Dry run">
          <div className="flex flex-col gap-2">
            <label className="text-xs text-slate-400" htmlFor="tool-task">
              Task id (ledger linkage)
            </label>
            <input
              id="tool-task"
              value={taskId}
              onChange={(event) => {
                setTaskId(event.target.value);
              }}
              className="rounded-md border border-white/10 bg-black/30 px-2 py-1 text-sm"
            />
            <label className="text-xs text-slate-400" htmlFor="tool-params">
              Params (JSON)
            </label>
            <textarea
              id="tool-params"
              value={paramsText}
              onChange={(event) => {
                setParamsText(event.target.value);
              }}
              rows={8}
              spellCheck={false}
              className="rounded-md border border-white/10 bg-black/30 px-2 py-1 font-mono text-xs"
            />
            <p className="text-xs text-slate-500">
              Dry-run is the default for risky tools: no side effects, policy
              evaluated, receipt stored as evidence.
            </p>
          </div>
        </GlassPanel>
      </div>
      <ToolRagPanel
        onSelectTool={(toolId) => {
          setSelectedId(toolId);
          const row = catalog.find((tool) => tool.id === toolId);
          selectEntity({
            kind: "system",
            id: toolId,
            label: row?.name ?? toolId,
            detail: row?.description ?? "Ranked by local Tool-RAG",
          });
        }}
      />
      <GlassPanel label="Receipt">
        {receipt === null ? (
          <EmptyState
            title="No receipt yet"
            message="Dry-run or invoke a tool to see the policy decision and output."
          />
        ) : (
          <div className="flex flex-col gap-2">
            <div className="flex items-center justify-between gap-3">
              <div>
                <p className="font-mono text-xs text-slate-500">{receipt.id}</p>
                <p className="mt-1 text-sm text-slate-200">
                  {receipt.policyReason}
                </p>
              </div>
              <StatusPill tone={STATUS_TONE[receipt.status] ?? "neutral"}>
                {receipt.status}
              </StatusPill>
            </div>
            <KeyValueList
              entries={[
                { key: "tool", value: receipt.toolId },
                { key: "policy", value: receipt.policyDecision },
                { key: "dry run", value: String(receipt.dryRun) },
                { key: "evidence", value: receipt.evidenceId ?? "—" },
                {
                  key: "compensation",
                  value: receipt.compensationPlanId ?? "—",
                },
                {
                  key: "error",
                  value: receipt.error
                    ? `${receipt.error.code}: ${receipt.error.message}`
                    : "—",
                },
              ]}
            />
            <pre className="max-h-64 overflow-auto rounded-md border border-white/10 bg-black/40 p-2 font-mono text-xs text-slate-200">
              {JSON.stringify(receipt.output, null, 2)}
            </pre>
          </div>
        )}
      </GlassPanel>
    </div>
  );
}
