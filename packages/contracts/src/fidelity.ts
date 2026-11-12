import { z } from "zod";

/**
 * Execution fidelity: the typed distinction between simulation and reality.
 * Every Talos layer (agents, tools, ledger, UI, audit swarm) uses these
 * values — no layer may redefine them, and no layer may silently upgrade
 * a mock result to live or verified. `live_remote` marks real execution on
 * a BYOC runner: never confusable with local `live`.
 */
export const ExecutionFidelitySchema = z.enum([
  "mock",
  "live",
  "live_remote",
  "verified",
]);
export type ExecutionFidelity = z.infer<typeof ExecutionFidelitySchema>;

/** Fidelity rank order (mock < live < live_remote < verified). */
export function fidelityRank(fidelity: ExecutionFidelity): number {
  switch (fidelity) {
    case "mock":
      return 0;
    case "live":
      return 1;
    case "live_remote":
      return 2;
    case "verified":
      return 3;
  }
}

/** The weaker of two fidelities (a parent is only as verified as its least-verified child). */
export function minFidelity(
  a: ExecutionFidelity,
  b: ExecutionFidelity,
): ExecutionFidelity {
  return fidelityRank(a) <= fidelityRank(b) ? a : b;
}

/** Thrown when a result is presented above what the work actually backs. */
export class FidelityMismatchError extends Error {
  readonly actual: ExecutionFidelity;
  readonly claimed: ExecutionFidelity;

  constructor(actual: ExecutionFidelity, claimed: ExecutionFidelity) {
    super(
      `fidelity mismatch: ${actual} work presented as ${claimed} (actual: ${actual})`,
    );
    this.name = "FidelityMismatchError";
    this.actual = actual;
    this.claimed = claimed;
  }
}

/**
 * Reject claims above what the work actually backs. `claimed <= actual`
 * passes; anything stronger throws {@link FidelityMismatchError}.
 */
export function assertFidelity(
  actual: ExecutionFidelity,
  claimed: ExecutionFidelity,
): void {
  if (fidelityRank(claimed) > fidelityRank(actual)) {
    throw new FidelityMismatchError(actual, claimed);
  }
}
