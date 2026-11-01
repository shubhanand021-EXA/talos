/**
 * @talos/contracts — single source of truth for Talos cross-layer types.
 *
 * Every layer (React, Rust policy broker, Tauri bridge, audit swarm,
 * evidence ledger) imports these schemas. No layer may redefine them.
 */
export const CONTRACTS_VERSION = "0.3.0" as const;

export * from "./identity.js";
export * from "./agent.js";
export * from "./task.js";
export * from "./delegation.js";
export * from "./tool.js";
export * from "./browser.js";
export * from "./computer.js";
export * from "./context.js";
export * from "./toolrag.js";
export * from "./llm.js";
export * from "./policy.js";
export * from "./evidence.js";
export * from "./compensation.js";
export * from "./approval.js";
export * from "./system.js";
export * from "./ipc.js";
export * from "./workspace.js";
export * from "./memory.js";
export * from "./ops.js";
export * from "./storage.js";
export * from "./fidelity.js";
export * from "./guards.js";
