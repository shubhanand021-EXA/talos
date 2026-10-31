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

const RISK_TONE: Record<string, PillTone> = {
  Low: "ok",
  low: "ok",
  Medium: "warn",
  medium: "warn",
  High: "error",
  high: "error",
};

const STATUS_TONE: Record<string, PillTone> = {
  success: "ok",
  failed: "error",
  denied: "warn",
  disabled: "neutral",
};

const BROWSER_ACTIONS = [
  "browser.navigate",
  "browser.snapshot",
  "browser.extract",
  "browser.screenshot",
  "browser.scroll",
  "browser.click",
  "browser.type",
  "browser.select",
] as const;

const PARAM_TEMPLATES: Record<string, string> = {
  "browser.navigate": `{\n  "url": "https://example.com"\n}`,
  "browser.snapshot": `{\n  "url": "https://example.com"\n}`,
  "browser.extract": `{\n  "url": "https://example.com",\n  "maxChars": 5000\n}`,
  "browser.screenshot": `{\n  "url": "https://example.com",\n  "fullPage": false\n}`,
  "browser.scroll": `{\n  "url": "https://example.com",\n  "direction": "down",\n  "pixels": 800\n}`,
  "browser.click": `{\n  "url": "https://example.com",\n  "selector": "button#ok"\n}`,
  "browser.type": `{\n  "url": "https://example.com",\n  "selector": "input#q",\n  "text": "hello"\n}`,
  "browser.select": `{\n  "url": "https://example.com",\n  "selector": "select#lang",\n  "value": "en"\n}`,
};

const HIGH_RISK_KINDS = [
  "form_submit",
  "purchase",
  "settings_change",
  "file_upload",
  "file_download",
  "destructive",
  "other",
];

export function BrowserPage() {
  const selectEntity = useUiStore((state) => state.selectEntity);
  const [catalog, setCatalog] = useState<ToolManifest[]>([]);
  const [selectedId, setSelectedId] = useState<string>("browser.snapshot");
  const [manifest, setManifest] = useState<ToolManifest | null>(null);
  const [taskId, setTaskId] = useState("task-browser-1");
  const [paramsText, setParamsText] = useState(
    PARAM_TEMPLATES["browser.snapshot"] ?? "{}",
  );
  const [highRisk, setHighRisk] = useState(false);
  const [highRiskKind, setHighRiskKind] = useState("form_submit");
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
        const browserTools = tools.filter((tool) =>
          tool.id.startsWith("browser."),
        );
        if (!cancelled) {
          setCatalog(browserTools);
          setNotice(null);
          if (!browserTools.some((tool) => tool.id === selectedId)) {
            setSelectedId(browserTools[0]?.id ?? "browser.snapshot");
          }
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
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    let cancelled = false;
    const load = async () => {
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

  const parseParams = (): Record<string, unknown> => {
    const params = JSON.parse(paramsText) as Record<string, unknown>;
    if (highRisk) {
      return { ...params, highRisk: true, highRiskKind };
    }
    return params;
  };

  const runInvocation = async (dryRun: boolean) => {
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
        label: `Browser ${result.status}: ${selectedId}`,
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
  const showHighRiskWarning =
    highRisk ||
    selected?.id === "browser.click" ||
    selected?.id === "browser.type";

  return (
    <div className="flex flex-col gap-4">
      <SectionHeader
        title="Browser"
        subtitle={
          loading
            ? "Loading catalog…"
            : `${catalog.length} browser tools · dry-run first · isolated profile`
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
      <GlassPanel label="Target">
        <div className="flex flex-col gap-2">
          <label className="text-xs text-slate-400" htmlFor="browser-action">
            Action
          </label>
          <select
            id="browser-action"
            value={selectedId}
            onChange={(event) => {
              setSelectedId(event.target.value);
            }}
            className="rounded-md border border-white/10 bg-black/30 px-2 py-1 text-sm"
          >
            {BROWSER_ACTIONS.map((id) => (
              <option key={id} value={id}>
                {id}
              </option>
            ))}
          </select>
          <label className="text-xs text-slate-400" htmlFor="browser-task">
            Task id (ledger linkage)
          </label>
          <input
            id="browser-task"
            value={taskId}
            onChange={(event) => {
              setTaskId(event.target.value);
            }}
            className="rounded-md border border-white/10 bg-black/30 px-2 py-1 text-sm"
          />
          <label className="text-xs text-slate-400" htmlFor="browser-params">
            Params (JSON)
          </label>
          <textarea
            id="browser-params"
            value={paramsText}
            onChange={(event) => {
              setParamsText(event.target.value);
            }}
            rows={7}
            spellCheck={false}
            className="rounded-md border border-white/10 bg-black/30 px-2 py-1 font-mono text-xs"
          />
          <label className="flex items-center gap-2 text-xs text-slate-300">
            <input
              type="checkbox"
              checked={highRisk}
              onChange={(event) => {
                setHighRisk(event.target.checked);
              }}
            />
            High-risk action (form submit, purchase, settings, upload/download,
            destructive)
          </label>
          {highRisk ? (
            <select
              aria-label="High-risk kind"
              value={highRiskKind}
              onChange={(event) => {
                setHighRiskKind(event.target.value);
              }}
              className="rounded-md border border-amber-300/30 bg-black/30 px-2 py-1 text-sm"
            >
              {HIGH_RISK_KINDS.map((kind) => (
                <option key={kind} value={kind}>
                  {kind}
                </option>
              ))}
            </select>
          ) : null}
          <p className="text-xs text-slate-500">
            Mutations resolve the selector first: dry-run validates without
            touching the page. Clicks, types, and selects need recorded
            evidence; high-risk runs need human approval.
          </p>
        </div>
      </GlassPanel>
      {showHighRiskWarning ? (
        <GlassPanel label="Safety warning">
          <p role="alert" className="text-xs text-amber-300">
            Consequential action: dry-run first, confirm the resolved selector,
            and expect a human-review gate for high-risk flags. Destructive web
            actions need explicit approval — automation never proceeds silently.
          </p>
        </GlassPanel>
      ) : null}
      <GlassPanel label="Tool catalog">
        {catalog.length === 0 ? (
          <EmptyState
            title="No browser tools yet"
            message="Launch the desktop shell to load the live registry."
          />
        ) : (
          <DataTable<ToolManifest>
            caption="Browser tool catalog"
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
                header: "Evidence",
                render: (row) => (row.requiresEvidence ? "required" : "—"),
              },
            ]}
          />
        )}
      </GlassPanel>
      <GlassPanel label="Receipt">
        {receipt === null ? (
          <EmptyState
            title="No receipt yet"
            message="Dry-run or invoke an action to see the policy decision, hashes, and output."
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
            <div className="grid grid-cols-1 gap-2 xl:grid-cols-2">
              <KeyValueList
                entries={[
                  {
                    key: "dom hash",
                    value: hashOf(receipt.output, "domHash"),
                  },
                  {
                    key: "screenshot hash",
                    value: hashOf(receipt.output, "screenshotHash"),
                  },
                  {
                    key: "extracted text hash",
                    value: hashOf(receipt.output, "textHash"),
                  },
                ]}
              />
              <pre className="max-h-64 overflow-auto rounded-md border border-white/10 bg-black/40 p-2 font-mono text-xs text-slate-200">
                {JSON.stringify(receipt.output, null, 2)}
              </pre>
            </div>
          </div>
        )}
      </GlassPanel>
      {selected !== null ? (
        <GlassPanel label="Manifest">
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
        </GlassPanel>
      ) : null}
    </div>
  );
}

function hashOf(output: unknown, key: string): string {
  if (typeof output !== "object" || output === null) {
    return "—";
  }
  const value = (output as Record<string, unknown>)[key];
  return typeof value === "string" ? value : "—";
}
