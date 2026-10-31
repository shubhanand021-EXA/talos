import { AgentDescriptorSchema, TaskRecordSchema } from "@talos/contracts";
import { create } from "zustand";
import type { ActivityEvent } from "../types/index.js";

const STAMP = "2026-10-31T12:00:00.000Z";

function seedAgents() {
  return [
    {
      id: "agent-orchestrator-1",
      displayName: "Orchestrator Prime",
      role: "orchestrator",
      status: "running",
      sessionId: "session-local",
      capabilities: ["plan", "delegate", "observe"],
      createdAt: STAMP,
      updatedAt: STAMP,
    },
    {
      id: "agent-coder-1",
      displayName: "Coder Unit 01",
      role: "coder",
      status: "running",
      sessionId: "session-local",
      taskId: "task-contracts",
      capabilities: ["fs.read", "fs.write", "test.run"],
      createdAt: STAMP,
      updatedAt: STAMP,
    },
    {
      id: "agent-auditor-1",
      displayName: "Audit Warden",
      role: "auditor",
      status: "idle",
      sessionId: "session-local",
      capabilities: ["evidence.read", "policy.read"],
      createdAt: STAMP,
      updatedAt: STAMP,
    },
  ].map((raw) => AgentDescriptorSchema.parse(raw));
}

function seedTasks() {
  return [
    {
      id: "task-contracts",
      sessionId: "session-local",
      title: "Stabilize shared contracts",
      description: "Freeze v0.1.0 of @talos/contracts for UI and broker use.",
      status: "executing",
      priority: "high",
      agentId: "agent-coder-1",
      createdAt: STAMP,
      updatedAt: STAMP,
    },
    {
      id: "task-policy",
      sessionId: "session-local",
      title: "Harden policy broker rules",
      description:
        "Default-deny evaluation with evidence and compensation gates.",
      status: "planning",
      priority: "critical",
      createdAt: STAMP,
      updatedAt: STAMP,
    },
    {
      id: "task-viewport",
      sessionId: "session-local",
      title: "Design spatial viewport",
      description: "Void Neon 3D topology arrives in a later batch.",
      status: "queued",
      priority: "normal",
      createdAt: STAMP,
      updatedAt: STAMP,
    },
  ].map((raw) => TaskRecordSchema.parse(raw));
}

const ACTIVITY: ActivityEvent[] = [
  {
    id: "evt-1",
    at: STAMP,
    actor: "policy-broker",
    message: "Allowed low-risk read on talos/contracts",
    tone: "success",
  },
  {
    id: "evt-2",
    at: STAMP,
    actor: "coder-1",
    message: "Requested write to packages/contracts/src",
    tone: "info",
  },
  {
    id: "evt-3",
    at: STAMP,
    actor: "policy-broker",
    message: "Held write behind evidence record ev-1042",
    tone: "warning",
  },
  {
    id: "evt-4",
    at: STAMP,
    actor: "audit-warden",
    message: "Verified evidence chain for claim-17",
    tone: "success",
  },
];

interface MockState {
  ready: boolean;
  hydrate: () => void;
}

const agents = seedAgents();
const tasks = seedTasks();

/**
 * Deterministic fixtures for UI development. No randomness, no network:
 * the same snapshot renders on every machine. Live data arrives in Prompt 05+.
 */
export function getMockAgents() {
  return agents;
}

export function getMockTasks() {
  return tasks;
}

export function getMockActivity(): ActivityEvent[] {
  return ACTIVITY;
}

export const useMockStore = create<MockState>()((set) => ({
  ready: false,
  hydrate: () => {
    set({ ready: true });
  },
}));
