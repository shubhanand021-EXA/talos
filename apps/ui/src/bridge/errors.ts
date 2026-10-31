/** Structured bridge failure. Never throws raw strings across the UI. */
export class BridgeError extends Error {
  readonly code: BridgeErrorCode;

  constructor(code: BridgeErrorCode, message: string) {
    super(message);
    this.name = "BridgeError";
    this.code = code;
  }
}

export type BridgeErrorCode =
  | "backend_unavailable"
  | "invoke_rejected"
  | "validation_failed"
  | "subscription_failed";

/** True when the error means "no Rust backend reachable". */
export function isBackendUnavailable(error: unknown): boolean {
  return error instanceof BridgeError && error.code === "backend_unavailable";
}
