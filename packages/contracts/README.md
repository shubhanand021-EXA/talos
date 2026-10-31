# @talos/contracts

Single source of truth for Talos cross-layer types: React, Rust policy broker, Tauri bridge, agent runtime, audit swarm, evidence ledger.

No layer may redefine these schemas. Import from here.

## Install

This package lives in the Talos monorepo (`packages/contracts`) and is linked via pnpm workspaces. No registry publish in Batch 1.

```sh
pnpm install
```

## Usage

```ts
import {
  AgentDescriptorSchema,
  ActionRequestSchema,
  IpcCommandName,
  validateWith,
} from "@talos/contracts";

// Parse at the boundary (Tauri bridge, IPC, forms)
const action = ActionRequestSchema.parse(raw);

// Or validate without throwing
const result = validateWith(AgentDescriptorSchema, raw);
if (!result.ok) {
  console.error(result.issues);
}

// Stable IPC names — never hardcode strings elsewhere
await invoke(IpcCommandName.requestAction, action);
```

## Export map

| Module         | Schemas                                                                                                               | Types (inferred)   |
| -------------- | --------------------------------------------------------------------------------------------------------------------- | ------------------ |
| `identity`     | `IsoDateTime`, `TalosId`, `AgentId`, `TaskId`, `SessionId`, `EvidenceId`, `CompensationPlanId`                        | Same names         |
| `agent`        | `AgentRole`, `AgentStatus`, `AgentDescriptor`                                                                         | Same names         |
| `task`         | `TaskStatus`, `TaskPriority`, `TaskRecord`                                                                            | Same names         |
| `policy`       | `RiskLevel`, `ActionRequest`, `PolicyDecision`, `PolicyEvaluation`                                                    | Same names         |
| `evidence`     | `EvidenceKind`, `EvidenceStatus`, `EvidenceRecord`, `ClaimRecord`                                                     | Same names         |
| `compensation` | `CompensationStrategy`, `CompensationStep`, `CompensationPlan`                                                        | Same names         |
| `system`       | `SystemStatus`, `HeartbeatEvent`, `SceneNode(Kind/Status)`, `SceneEdge(State)`, `AuditSeverity`, `AuditReportSummary` | Same names         |
| `ipc`          | `IpcCommandName`, `IpcEventName` (+ zod schemas)                                                                      | Same names         |
| `guards`       | `validateWith`, `isValidEnum`                                                                                         | `ValidationResult` |

Every schema is strict (`.strict()`): unknown keys are rejected.

Dates are ISO-8601 strings (`z.iso.datetime()`). Free-form payloads are `unknown` (never `any`).

## Versioning policy

- Batch 1: `0.1.0`, `private: true`, breaking changes allowed with a minor bump.
- Post-Batch 1: semver. Additive schema changes are minor; any breaking parse change is major.
- `CONTRACTS_VERSION` exported from the index always matches `package.json`.
- IPC name objects (`IpcCommandName`, `IpcEventName`) are append-only: never rename or remove a key in a minor release.
