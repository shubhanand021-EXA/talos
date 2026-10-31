import { invoke } from "@tauri-apps/api/core";
import type { z } from "zod";
import { BridgeError } from "./errors.js";

/** True only inside the Tauri container (native IPC available). */
export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function invokeErrorMessage(error: unknown): string {
  if (error instanceof BridgeError) {
    throw error;
  }
  if (typeof error === "string") {
    return error;
  }
  if (error instanceof Error) {
    return error.message;
  }
  return "Unknown backend failure";
}

/**
 * Typed `invoke` wrapper. Throws {@link BridgeError} when the backend is
 * unreachable, rejects, or returns a payload that fails `schema`.
 */
export async function invokeCommand<T>(
  command: string,
  args?: Record<string, unknown>,
  schema?: z.ZodType<T>,
): Promise<T> {
  if (!isTauri()) {
    throw new BridgeError(
      "backend_unavailable",
      `Tauri backend unavailable for command "${command}" (running in browser)`,
    );
  }
  let raw: unknown;
  try {
    raw = await invoke<unknown>(command, args);
  } catch (error) {
    throw new BridgeError("invoke_rejected", invokeErrorMessage(error));
  }
  if (schema === undefined) {
    return raw as T;
  }
  const parsed = schema.safeParse(raw);
  if (!parsed.success) {
    throw new BridgeError(
      "validation_failed",
      `Response for "${command}" failed validation: ${parsed.error.message}`,
    );
  }
  return parsed.data;
}
