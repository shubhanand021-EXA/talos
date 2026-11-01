import { useEffect } from "react";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { IpcEventName } from "@talos/contracts";
import {
  AgentSpawnedPayloadSchema,
  ChildTaskCompletedPayloadSchema,
  ChildTaskFailedPayloadSchema,
  ComputerSafetyChangedPayloadSchema,
  ContextCompactedPayloadSchema,
  DelegationSpawnPayloadSchema,
  DiagnosticTaskPayloadSchema,
  EvidenceUpdatedPayloadSchema,
  HeartbeatEventSchema,
  PolicyEvaluatedPayloadSchema,
  QueueTaskPayloadSchema,
  ResultsMergedPayloadSchema,
  getAppInfo,
  getComputerSafetyStatus,
  getSystemStatus,
  getTaskState,
  isTauri,
  subscribeTalosEvent,
  type AgentSpawnedPayload,
  type ChildTaskCompletedPayload,
  type ChildTaskFailedPayload,
  type ComputerSafetyChangedPayload,
  type ContextCompactedPayload,
  type DelegationSpawnPayload,
  type DiagnosticTaskPayload,
  type EvidenceUpdatedPayload,
  type HeartbeatEvent,
  type PolicyEvaluatedPayload,
  type QueueTaskPayload,
  type ResultsMergedPayload,
} from "./index.js";
import { isBackendUnavailable } from "./errors.js";
import { useAppStore } from "../stores/appStore.js";
import { useUiStore } from "../stores/uiStore.js";

/** Feed id suffix that stays stable for the session (timestamps differ). */
function feedId(prefix: string): string {
  return `${prefix}-${Date.now()}`;
}

/**
 * Backend sync loop. Mounted once by the shell: pulls the initial snapshot,
 * subscribes to the five lifecycle events, and funnels everything into the
 * Zustand stores. Outside Tauri (plain browser) it marks the link
 * disconnected and leaves the deterministic mock snapshot in place.
 */
export function useTalosSync(): void {
  useEffect(() => {
    if (!isTauri()) {
      useAppStore.getState().setConnectionState("disconnected");
      return;
    }
    let cancelled = false;
    const unlistens: UnlistenFn[] = [];

    const boot = async () => {
      try {
        const [info, status, computerSafety] = await Promise.all([
          getAppInfo(),
          getSystemStatus(),
          getComputerSafetyStatus().catch(() => null),
        ]);
        if (cancelled) {
          return;
        }
        const store = useAppStore.getState();
        store.setAppInfo(info);
        store.setSystemStatus(status);
        store.setComputerSafety(computerSafety);
        store.setConnectionState("connected");
      } catch (error) {
        if (!cancelled && isBackendUnavailable(error)) {
          useAppStore.getState().setConnectionState("disconnected");
        }
        return;
      }
      const guard = async (work: () => Promise<unknown>): Promise<void> => {
        try {
          await work();
        } catch {
          // A single failed subscription must not take the UI down.
        }
      };
      const track = (promise: Promise<UnlistenFn>): Promise<void> =>
        guard(async () => {
          unlistens.push(await promise);
        });
      await track(
        subscribeTalosEvent(
          IpcEventName.taskCreated,
          (payload: DiagnosticTaskPayload) => {
            useAppStore.getState().appendFeed({
              id: feedId("task"),
              at: new Date().toISOString(),
              actor: "runtime",
              message: `Task created: ${payload.task.title}`,
              tone: "info",
              taskId: payload.task.id,
            });
          },
          DiagnosticTaskPayloadSchema,
        ),
      );
      await track(
        subscribeTalosEvent(
          IpcEventName.taskUpdated,
          (payload: QueueTaskPayload) => {
            const store = useAppStore.getState();
            store.appendFeed({
              id: feedId("queue"),
              at: new Date().toISOString(),
              actor: "runtime",
              message: `Task ${payload.task.id} -> ${payload.task.status}`,
              tone: "info",
              taskId: payload.task.id,
            });
            void getTaskState(payload.task.id)
              .then((task) => {
                useAppStore.getState().upsertLiveTask(task);
              })
              .catch(() => {
                // Backend may have reset; list refresh covers it.
              });
          },
          QueueTaskPayloadSchema,
        ),
      );
      await track(
        subscribeTalosEvent(
          IpcEventName.agentSpawned,
          (payload: AgentSpawnedPayload) => {
            const store = useAppStore.getState();
            store.appendLiveAgent(payload.agent);
            store.appendFeed({
              id: feedId("agent"),
              at: new Date().toISOString(),
              actor: payload.agent.displayName,
              message: `Agent spawned (${payload.agent.role})`,
              tone: "success",
            });
          },
          AgentSpawnedPayloadSchema,
        ),
      );
      await track(
        subscribeTalosEvent(
          IpcEventName.policyEvaluated,
          (payload: PolicyEvaluatedPayload) => {
            const evaluation = payload.evaluation;
            const store = useAppStore.getState();
            store.setLastEvaluation(evaluation);
            store.appendFeed({
              id: feedId("policy"),
              at: new Date().toISOString(),
              actor: "policy-broker",
              message: `Policy ${evaluation.decision}: ${evaluation.reason}`,
              tone: evaluation.decision === "deny" ? "error" : "info",
            });
            useUiStore.getState().selectEntity({
              kind: "system",
              id: evaluation.id,
              label: `Policy ${evaluation.decision}`,
              detail: evaluation.reason,
            });
          },
          PolicyEvaluatedPayloadSchema,
        ),
      );
      await track(
        subscribeTalosEvent(
          IpcEventName.evidenceUpdated,
          (payload: EvidenceUpdatedPayload) => {
            const store = useAppStore.getState();
            store.bumpEvidenceVersion();
            store.appendFeed({
              id: feedId("evidence"),
              at: new Date().toISOString(),
              actor: "evidence-store",
              message: `Evidence recorded: ${payload.evidence.id} (${payload.evidence.kind})`,
              tone: "success",
            });
          },
          EvidenceUpdatedPayloadSchema,
        ),
      );
      await track(
        subscribeTalosEvent(
          IpcEventName.heartbeat,
          (payload: HeartbeatEvent) => {
            const store = useAppStore.getState();
            const current = store.systemStatus;
            store.setSystemStatus({
              ...current,
              uptimeSeconds: payload.uptimeSeconds ?? current.uptimeSeconds,
            });
            if (store.connectionState !== "connected") {
              store.setConnectionState("connected");
            }
          },
          HeartbeatEventSchema,
        ),
      );
      await track(
        subscribeTalosEvent(
          IpcEventName.computerSafetyChanged,
          (payload: ComputerSafetyChangedPayload) => {
            const store = useAppStore.getState();
            void getComputerSafetyStatus()
              .then((safety) => {
                useAppStore.getState().setComputerSafety(safety);
              })
              .catch(() => {
                const current = store.computerSafety;
                if (current !== null) {
                  useAppStore.getState().setComputerSafety({
                    ...current,
                    emergencyStopped: payload.emergencyStopped,
                  });
                }
              });
            useAppStore.getState().appendFeed({
              id: feedId("computer-safety"),
              at: new Date().toISOString(),
              actor: "computer-use",
              message: payload.emergencyStopped
                ? `EMERGENCY STOP engaged (${payload.canceledTasks} tasks canceled)`
                : "Emergency stop cleared",
              tone: payload.emergencyStopped ? "error" : "info",
            });
          },
          ComputerSafetyChangedPayloadSchema,
        ),
      );
      await track(
        subscribeTalosEvent(
          IpcEventName.contextCompacted,
          (payload: ContextCompactedPayload) => {
            useAppStore.getState().appendFeed({
              id: feedId("context"),
              at: new Date().toISOString(),
              actor: "context-budget",
              message: `TypeCompact ${payload.taskId}: ${payload.originalTokens} → ${payload.compactedTokens} tokens (${payload.proofHash})`,
              tone: "info",
            });
          },
          ContextCompactedPayloadSchema,
        ),
      );
      await track(
        subscribeTalosEvent(
          IpcEventName.agentDelegated,
          (payload: DelegationSpawnPayload) => {
            const store = useAppStore.getState();
            store.appendFeed({
              id: feedId("delegated"),
              at: new Date().toISOString(),
              actor: "supervisor",
              message: `Delegated ${payload.childTaskId} (${payload.role}) from ${payload.parentTaskId}`,
              tone: "info",
              taskId: payload.parentTaskId,
            });
            void getTaskState(payload.childTaskId)
              .then((task) => {
                useAppStore.getState().upsertLiveTask(task);
              })
              .catch(() => undefined);
          },
          DelegationSpawnPayloadSchema,
        ),
      );
      await track(
        subscribeTalosEvent(
          IpcEventName.childTaskSpawned,
          (payload: DelegationSpawnPayload) => {
            useAppStore.getState().appendFeed({
              id: feedId("child-spawn"),
              at: new Date().toISOString(),
              actor: "supervisor",
              message: `Child spawned: ${payload.childTaskId} (${payload.role})`,
              tone: "info",
              taskId: payload.childTaskId,
            });
          },
          DelegationSpawnPayloadSchema,
        ),
      );
      await track(
        subscribeTalosEvent(
          IpcEventName.childTaskCompleted,
          (payload: ChildTaskCompletedPayload) => {
            const store = useAppStore.getState();
            store.appendFeed({
              id: feedId("child-done"),
              at: new Date().toISOString(),
              actor: "supervisor",
              message: `Child completed: ${payload.childTaskId} -> ${payload.status}`,
              tone: "success",
              taskId: payload.childTaskId,
            });
            void getTaskState(payload.childTaskId)
              .then((task) => {
                useAppStore.getState().upsertLiveTask(task);
              })
              .catch(() => undefined);
          },
          ChildTaskCompletedPayloadSchema,
        ),
      );
      await track(
        subscribeTalosEvent(
          IpcEventName.childTaskFailed,
          (payload: ChildTaskFailedPayload) => {
            useAppStore.getState().appendFeed({
              id: feedId("child-fail"),
              at: new Date().toISOString(),
              actor: "supervisor",
              message: `Child failed: ${payload.childTaskId} (${payload.error})`,
              tone: "error",
              taskId: payload.childTaskId,
            });
          },
          ChildTaskFailedPayloadSchema,
        ),
      );
      await track(
        subscribeTalosEvent(
          IpcEventName.resultsMerged,
          (payload: ResultsMergedPayload) => {
            const store = useAppStore.getState();
            store.appendFeed({
              id: feedId("merged"),
              at: new Date().toISOString(),
              actor: "supervisor",
              message: `Results merged into ${payload.parentTaskId}: ${payload.status} (${payload.artifactCount} artifacts)`,
              tone: payload.status === "completed" ? "success" : "error",
              taskId: payload.parentTaskId,
            });
            void getTaskState(payload.parentTaskId)
              .then((task) => {
                useAppStore.getState().upsertLiveTask(task);
              })
              .catch(() => undefined);
          },
          ResultsMergedPayloadSchema,
        ),
      );
    };

    void boot();
    return () => {
      cancelled = true;
      for (const unlisten of unlistens) {
        unlisten();
      }
    };
  }, []);
}
