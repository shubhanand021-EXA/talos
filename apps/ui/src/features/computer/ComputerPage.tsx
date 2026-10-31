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
  computerEmergencyStop,
  computerResetStop,
  describeTool,
  getComputerSafetyStatus,
  invokeTool,
  invokeToolDryRun,
  isBackendUnavailable,
  listTools,
} from "../../bridge/index.js";
import { useAppStore } from "../../stores/appStore.js";
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

const COMPUTER_ACTIONS = [
  "computer.move_mouse",
  "computer.click",
  "computer.double_click",
  "computer.drag",
  "computer.type_text",
  "computer.hotkey",
  "computer.scroll",
  "computer.capture_screen",
  "computer.locate_element",
] as const;

const PARAM_TEMPLATES: Record<string, string> = {
  "computer.move_mouse": `{\n  "x": 640,\n  "y": 360\n}`,
  "computer.click": `{\n  "selector": "button#ok"\n}`,
  "computer.double_click": `{\n  "x": 640,\n  "y": 480\n}`,
  "computer.drag": `{\n  "x": 100,\n  "y": 100,\n  "x2": 400,\n  "y2": 300\n}`,
  "computer.type_text": `{\n  "selector": "input#q",\n  "text": "hello"\n}`,
  "computer.hotkey": `{\n  "keys": "ctrl+s"\n}`,
  "computer.scroll": `{\n  "direction": "down",\n  "pixels": 800\n}`,
  "computer.capture_screen": `{}`,
  "computer.locate_element": `{\n  "selector": "button#ok"\n}`,
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

/** Mock virtual screen: fixed elements + the last resolved bounding box. */
function MockScreen({
  selection,
  receiptBox,
}: {
  selection: { x?: number; y?: number } | null;
  receiptBox:
    { x: number; y: number; width: number; height: number } | null | undefined;
}) {
  const scaleX = 100 / 1440;
  const scaleY = 100 / 900;
  const box = (x: number, y: number, w: number, h: number) => ({
    x: `${x * scaleX}%`,
    y: `${y * scaleY}%`,
    width: `${w * scaleX}%`,
    height: `${h * scaleY}%`,
  });
  const fixed: Array<{ label: string; rect: ReturnType<typeof box> }> = [
    { label: "window", rect: box(40, 40, 1360, 800) },
    { label: "button#ok", rect: box(640, 480, 160, 48) },
    { label: "input#q", rect: box(560, 400, 320, 40) },
  ];
  return (
    <svg
      viewBox="0 0 100 62.5"
      role="img"
      aria-label="Mock virtual screen"
      className="h-auto w-full rounded-md border border-white/10 bg-black/40"
    >
      <rect x="0" y="0" width="100" height="62.5" fill="#05070d" />
      <rect x="0" y="60" width="100" height="2.5" fill="#0a0f1c" />
      <text x="1" y="61.8" fontSize="1.6" fill="#5b6b88">
        taskbar (no interaction)
      </text>
      {fixed.map((element) => (
        <g key={element.label}>
          <rect
            x={element.rect.x}
            y={element.rect.y}
            width={element.rect.width}
            height={element.rect.height}
            fill="none"
            stroke="#1a2333"
            strokeWidth="0.2"
          />
          <text
            x={element.rect.x}
            y={Number.parseFloat(element.rect.y) + 2}
            fontSize="1.6"
            fill="#5b6b88"
          >
            {element.label}
          </text>
        </g>
      ))}
      {selection?.x !== undefined && selection.y !== undefined ? (
        <circle
          cx={`${selection.x * scaleX}`}
          cy={`${selection.y * scaleY}`}
          r="1"
          fill="none"
          stroke="#00e5ff"
          strokeWidth="0.3"
        />
      ) : null}
      {receiptBox !== null && receiptBox !== undefined ? (
        <rect
          x={`${receiptBox.x * scaleX}%`}
          y={`${receiptBox.y * scaleY}%`}
          width={`${receiptBox.width * scaleX}%`}
          height={`${receiptBox.height * scaleY}%`}
          fill="none"
          stroke="#00ff9d"
          strokeWidth="0.3"
          strokeDasharray="1 0.5"
        />
      ) : null}
    </svg>
  );
}

export function ComputerPage() {
  const selectEntity = useUiStore((state) => state.selectEntity);
  const computerSafety = useAppStore((state) => state.computerSafety);
  const setComputerSafety = useAppStore((state) => state.setComputerSafety);
  const [catalog, setCatalog] = useState<ToolManifest[]>([]);
  const [selectedId, setSelectedId] = useState<string>(
    "computer.capture_screen",
  );
  const [manifest, setManifest] = useState<ToolManifest | null>(null);
  const [taskId, setTaskId] = useState("task-computer-1");
  const [paramsText, setParamsText] = useState(
    PARAM_TEMPLATES["computer.capture_screen"] ?? "{}",
  );
  const [highRisk, setHighRisk] = useState(false);
  const [highRiskKind, setHighRiskKind] = useState("destructive");
  const [receipt, setReceipt] = useState<ToolReceipt | null>(null);
  const [running, setRunning] = useState(false);
  const [loading, setLoading] = useState(false);
  const [stopping, setStopping] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    const refresh = async () => {
      setLoading(true);
      try {
        const [tools, safety] = await Promise.all([
          listTools(),
          getComputerSafetyStatus().catch(() => null),
        ]);
        if (!cancelled) {
          setCatalog(tools.filter((tool) => tool.id.startsWith("computer.")));
          if (safety !== null) {
            setComputerSafety(safety);
          }
          setNotice(null);
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

  const parseSelection = (): { x?: number; y?: number } | null => {
    try {
      const params = JSON.parse(paramsText) as Record<string, unknown>;
      const x = typeof params["x"] === "number" ? params["x"] : undefined;
      const y = typeof params["y"] === "number" ? params["y"] : undefined;
      if (x === undefined || y === undefined) {
        return null;
      }
      return { x, y };
    } catch {
      return null;
    }
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
        label: `Computer ${result.status}: ${selectedId}`,
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

  const runEmergency = async (reset: boolean) => {
    setStopping(true);
    setNotice(null);
    try {
      const status = reset
        ? await computerResetStop()
        : await computerEmergencyStop();
      setComputerSafety(status);
      selectEntity({
        kind: "system",
        id: `computer-safety-${Date.now()}`,
        label: reset ? "Emergency stop cleared" : "EMERGENCY STOP engaged",
        detail: `Stopped: ${status.emergencyStopped}. ${status.actionsThisMinute}/${status.maxActionsPerMinute} actions in the last minute.`,
      });
    } catch (error) {
      setNotice(
        isBackendUnavailable(error)
          ? "Backend unavailable — launch the desktop shell."
          : error instanceof Error
            ? error.message
            : "Emergency control failed",
      );
    } finally {
      setStopping(false);
    }
  };

  const selected =
    manifest ?? catalog.find((tool) => tool.id === selectedId) ?? null;
  const stopped = computerSafety?.emergencyStopped === true;
  const showHighRiskWarning =
    highRisk ||
    selectedId === "computer.type_text" ||
    selectedId === "computer.hotkey" ||
    selectedId === "computer.drag";
  const receiptBox =
    receipt !== null &&
    typeof receipt.output === "object" &&
    receipt.output !== null
      ? (((receipt.output as Record<string, unknown>)["boundingBox"] as
          | { x: number; y: number; width: number; height: number }
          | undefined) ?? null)
      : null;
  const receiptWarnings =
    receipt !== null &&
    typeof receipt.output === "object" &&
    receipt.output !== null
      ? (((receipt.output as Record<string, unknown>)["warnings"] as
          string[] | undefined) ?? [])
      : [];

  return (
    <div className="flex flex-col gap-4">
      <SectionHeader
        title="Computer"
        subtitle={
          loading
            ? "Loading catalog…"
            : `${catalog.length} computer tools · dry-run first · all disabled by default`
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
      <GlassPanel label="Safety status">
        {computerSafety === null ? (
          <EmptyState
            title="Safety status unknown"
            message="Launch the desktop shell to read the live envelope."
          />
        ) : (
          <div className="flex flex-col gap-3">
            <div className="flex flex-wrap items-center gap-2">
              <StatusPill tone={computerSafety.enabled ? "warn" : "neutral"}>
                {computerSafety.enabled ? "enabled" : "disabled by default"}
              </StatusPill>
              {stopped ? (
                <StatusPill tone="error">EMERGENCY STOPPED</StatusPill>
              ) : (
                <StatusPill tone="ok">stop clear</StatusPill>
              )}
              <StatusPill tone="info">
                {`rate ${computerSafety.actionsThisMinute}/${computerSafety.maxActionsPerMinute}/min`}
              </StatusPill>
            </div>
            <KeyValueList
              entries={[
                {
                  key: "dry run default",
                  value: String(computerSafety.dryRunDefault),
                },
                {
                  key: "viewport",
                  value: `${computerSafety.viewportWidth}x${computerSafety.viewportHeight}`,
                },
                {
                  key: "min confidence",
                  value: String(computerSafety.minConfidence),
                },
              ]}
            />
            <div className="flex gap-2">
              <NeonButton
                onClick={() => {
                  void runEmergency(false);
                }}
                loading={stopping}
              >
                Emergency stop
              </NeonButton>
              <NeonButton
                onClick={() => {
                  void runEmergency(true);
                }}
                loading={stopping}
              >
                Reset stop
              </NeonButton>
            </div>
            <p className="text-xs text-slate-500">
              The stop latch blocks every new computer action — including
              dry-runs — and cancels in-flight queue tasks. Reset is explicit
              and never resumes canceled tasks.
            </p>
          </div>
        )}
      </GlassPanel>
      {showHighRiskWarning ? (
        <GlassPanel label="Safety warning">
          <p role="alert" className="text-xs text-amber-300">
            Consequential action: dry-run first, confirm the target, and expect
            a human-review gate for high-risk flags. Destructive actions need
            explicit approval and can never run silently.
          </p>
        </GlassPanel>
      ) : null}
      <GlassPanel label="Target">
        <div className="flex flex-col gap-2">
          <label className="text-xs text-slate-400" htmlFor="computer-action">
            Action
          </label>
          <select
            id="computer-action"
            value={selectedId}
            onChange={(event) => {
              setSelectedId(event.target.value);
            }}
            className="rounded-md border border-white/10 bg-black/30 px-2 py-1 text-sm"
          >
            {COMPUTER_ACTIONS.map((id) => (
              <option key={id} value={id}>
                {id}
              </option>
            ))}
          </select>
          <label className="text-xs text-slate-400" htmlFor="computer-task">
            Task id (ledger linkage)
          </label>
          <input
            id="computer-task"
            value={taskId}
            onChange={(event) => {
              setTaskId(event.target.value);
            }}
            className="rounded-md border border-white/10 bg-black/30 px-2 py-1 text-sm"
          />
          <label className="text-xs text-slate-400" htmlFor="computer-params">
            Params (JSON, sandboxed to the virtual screen)
          </label>
          <textarea
            id="computer-params"
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
            Coordinates are validated against the virtual screen; settings and
            password managers are hard-blocked; terminals and payment flows
            warn. Real OS control is not implemented — the mock simulates.
          </p>
        </div>
      </GlassPanel>
      <div className="grid grid-cols-1 gap-4 2xl:grid-cols-2">
        <GlassPanel label="Mock screen">
          <MockScreen selection={parseSelection()} receiptBox={receiptBox} />
          <p className="mt-2 text-xs text-slate-500">
            Virtual 1440x900 desktop. Green box: last resolved bounding box.
            Cyan dot: requested coordinates.
          </p>
        </GlassPanel>
        <GlassPanel label="Tool catalog">
          {catalog.length === 0 ? (
            <EmptyState
              title="No computer tools yet"
              message="Launch the desktop shell to load the live registry."
            />
          ) : (
            <DataTable<ToolManifest>
              caption="Computer tool catalog"
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
              ]}
            />
          )}
        </GlassPanel>
      </div>
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
            {receiptWarnings.length > 0 ? (
              <p role="note" className="text-xs text-amber-300">
                {receiptWarnings.join(" · ")}
              </p>
            ) : null}
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
