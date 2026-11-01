import { Suspense, lazy, useEffect } from "react";
import { useAppStore } from "../../stores/appStore.js";
import {
  getMockActivity,
  getMockAgents,
  getMockTasks,
} from "../../stores/mockStore.js";
import { useApprovalStore } from "../../stores/approvalStore.js";
import { useUiStore } from "../../stores/uiStore.js";
import {
  GlassPanel,
  LogStream,
  NeonButton,
  SectionHeader,
  Spinner,
  StatusPill,
  type PillTone,
} from "../../design-system/index.js";
import { isBackendUnavailable, isTauri, runDiagnostic } from "../../bridge/index.js";
import { formatUptime } from "../../lib/format.js";
import { TaskQueueTable } from "../agents/TaskQueueTable.js";
import {
  ApprovalQueuePanel,
  useApprovalActions,
} from "../approval/index.js";
import { getMockApprovals } from "../approval/mockApprovals.js";

const VoidNeonViewport = lazy(() =>
  import("../void-neon/index.js").then((module) => ({
    default: module.VoidNeonViewport,
  })),
);

export function DashboardPage() {
  const {
    systemStatus,
    connectionState,
    liveFeed,
    lastEvaluation,
    diagnosticRunning,
    setDiagnosticRunning,
    setConnectionState,
    appendFeed,
    liveAgents,
    liveTasks,
  } = useAppStore();
  const selectEntity = useUiStore((state) => state.selectEntity);
  const selectedEntity = useUiStore((state) => state.selectedEntity);
  const pending = useApprovalStore((s) => s.pending);
  const setPending = useApprovalStore((s) => s.setPending);
  const { batchApprove, busy } = useApprovalActions();
  useEffect(() => {
    // Seed one preview approval only outside Tauri; live events own the
    // queue inside the desktop shell.
    if (!isTauri() && pending.length === 0) setPending(getMockApprovals().slice(0, 1));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  const agents = getMockAgents();
  const tasks = getMockTasks();
  const selectedTaskId =
    selectedEntity !== null && selectedEntity.kind === "task"
      ? selectedEntity.id
      : null;
  const activity =
    selectedTaskId === null
      ? [...liveFeed, ...getMockActivity()]
      : liveFeed.filter((event) => event.taskId === selectedTaskId);
  const running = agents.filter((agent) => agent.status === "running").length;
  const backendDown = connectionState === "disconnected";

  const handleRunDiagnostic = async () => {
    setDiagnosticRunning(true);
    try {
      const summary = await runDiagnostic();
      appendFeed({
        id: `diag-${Date.now()}`,
        at: new Date().toISOString(),
        actor: "runtime",
        message: `Diagnostic complete: ${summary.passed}/${summary.total} checks passed`,
        tone: summary.passed === summary.total ? "success" : "warning",
      });
      selectEntity({
        kind: "system",
        id: summary.evaluation.id,
        label: `Policy ${summary.evaluation.decision}`,
        detail: summary.evaluation.reason,
      });
    } catch (error) {
      if (isBackendUnavailable(error)) {
        setConnectionState("disconnected");
      }
      appendFeed({
        id: `diag-err-${Date.now()}`,
        at: new Date().toISOString(),
        actor: "runtime",
        message:
          error instanceof Error
            ? `Diagnostic failed: ${error.message}`
            : "Diagnostic failed",
        tone: "error",
      });
    } finally {
      setDiagnosticRunning(false);
    }
  };

  const cards: Array<{
    label: string;
    value: string;
    tone: PillTone;
    detail: string;
  }> = [
    {
      label: "Policy broker",
      value: systemStatus.policyBrokerHealthy ? "Healthy" : "Down",
      tone: systemStatus.policyBrokerHealthy ? "ok" : "error",
      detail: "Context-gated default-deny engine",
    },
    {
      label: "Evidence ledger",
      value: systemStatus.evidenceLedgerHealthy ? "Healthy" : "Down",
      tone: systemStatus.evidenceLedgerHealthy ? "ok" : "error",
      detail: "SQLite store · append-only",
    },
    {
      label: "Active agents",
      value: String(systemStatus.activeAgents),
      tone: "info",
      detail: backendDown
        ? `${running} running (mock roster)`
        : `${liveAgents.length} live via events · backend reports ${systemStatus.activeAgents}`,
    },
    {
      label: "Active tasks",
      value: String(systemStatus.activeTasks),
      tone: "info",
      detail: backendDown
        ? `${tasks.length} planned (mock) · uptime ${formatUptime(systemStatus.uptimeSeconds)}`
        : `${liveTasks.length} live · backend reports ${systemStatus.activeTasks} · uptime ${formatUptime(systemStatus.uptimeSeconds)}`,
    },
  ];

  return (
    <div className="flex flex-col gap-4">
      <SectionHeader
        title="Dashboard"
        subtitle={
          backendDown
            ? "Backend unreachable — showing deterministic mock snapshot"
            : "Fleet overview — live control plane"
        }
        action={
          <NeonButton
            onClick={() => {
              void handleRunDiagnostic();
            }}
            loading={diagnosticRunning}
          >
            Run Diagnostic
          </NeonButton>
        }
      />
      {backendDown ? (
        <GlassPanel label="Backend status">
          <SectionHeader
            title="Running in browser preview"
            subtitle="Tauri IPC is unavailable here. Launch the desktop shell for the live loop."
          />
        </GlassPanel>
      ) : null}
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 2xl:grid-cols-4">
        {cards.map((card) => (
          <GlassPanel key={card.label} label={card.label}>
            <p className="text-xs uppercase tracking-wider text-slate-500">
              {card.label}
            </p>
            <div className="mt-2 flex items-center justify-between">
              <span className="font-display text-2xl font-semibold">
                {card.value}
              </span>
              <StatusPill tone={card.tone}>{card.value}</StatusPill>
            </div>
            <p className="mt-2 text-xs text-slate-500">{card.detail}</p>
          </GlassPanel>
        ))}
      </div>
      {lastEvaluation !== null ? (
        <GlassPanel label="Latest policy evaluation">
          <div className="flex items-center justify-between gap-3">
            <div>
              <p className="font-mono text-xs text-slate-500">
                {lastEvaluation.id}
              </p>
              <p className="mt-1 text-sm text-slate-200">
                {lastEvaluation.reason}
              </p>
            </div>
            <StatusPill
              tone={lastEvaluation.decision === "deny" ? "error" : "info"}
            >
              {lastEvaluation.decision}
            </StatusPill>
          </div>
        </GlassPanel>
      ) : null}
      <div className="grid grid-cols-1 gap-4 2xl:grid-cols-2">
        <GlassPanel label="Recent activity">
          <SectionHeader
            title="Recent activity"
            subtitle={
              selectedTaskId === null
                ? "Newest first"
                : `Filtered by ${selectedTaskId} · select nothing to clear`
            }
          />
          <LogStream events={activity} />
        </GlassPanel>
        <GlassPanel label="Spatial viewport" className="overflow-hidden p-2">
          <Suspense
            fallback={
              <div className="flex h-[420px] items-center justify-center">
                <Spinner label="Igniting viewport" />
              </div>
            }
          >
            <VoidNeonViewport />
          </Suspense>
        </GlassPanel>
      </div>
      <GlassPanel label="Live task queue">
        <SectionHeader
          title="Live task queue"
          subtitle={`${liveTasks.length} tasks · updates from runtime events · manage in Agents`}
        />
        <TaskQueueTable tasks={liveTasks} />
      </GlassPanel>
      <GlassPanel label="Approval queue">
        <SectionHeader
          title="Approval queue"
          subtitle={`${pending.length} waiting · batchable low-risk, individual high-risk · amber pulses until decided`}
        />
        <ApprovalQueuePanel
          onBatchApprove={(taskId) => {
            void batchApprove(taskId);
          }}
          busy={busy}
        />
      </GlassPanel>
    </div>
  );
}
