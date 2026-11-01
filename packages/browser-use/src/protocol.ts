/**
 * Line-delimited JSON-RPC framing for the browser worker.
 *
 * Two modes share one code path:
 * - single-shot: one request object on argv/stdin, one receipt on stdout.
 * - serve (`--serve`): NDJSON request/response stream for a persistent worker,
 *   plus async `{ event, payload }` frames (auth-required, session-closed,
 *   worker errors).
 */
import { z } from "zod";

/** JSON-RPC request frame. */
export const RpcRequestSchema = z
  .object({
    id: z.string().min(1).max(128),
    method: z.string().min(1).max(128),
    params: z.unknown().optional(),
  })
  .strict();
export type RpcRequest = z.infer<typeof RpcRequestSchema>;

/** JSON-RPC success frame. */
export const RpcResponseSchema = z
  .object({
    id: z.string().min(1).max(128),
    ok: z.boolean(),
    result: z.unknown().optional(),
    error: z
      .object({
        code: z.string().min(1).max(64),
        message: z.string().min(1).max(2048),
      })
      .strict()
      .optional(),
  })
  .strict();
export type RpcResponse = z.infer<typeof RpcResponseSchema>;

/** Async event frame emitted outside any request. */
export const RpcEventSchema = z
  .object({
    event: z.string().min(1).max(128),
    payload: z.unknown().optional(),
  })
  .strict();
export type RpcEvent = z.infer<typeof RpcEventSchema>;

/** Worker lifecycle events (mirrored to Tauri where wired). */
export const WorkerEventName = {
  browserAuthRequired: "browserAuthRequired",
  browserWorkerCrashed: "browserWorkerCrashed",
  browserSessionClosed: "browserSessionClosed",
} as const;
export type WorkerEventName =
  (typeof WorkerEventName)[keyof typeof WorkerEventName];

/** Encode one frame (single trailing newline, no pretty-print). */
export function encodeFrame(value: unknown): string {
  return `${JSON.stringify(value)}\n`;
}

/**
 * Split a byte stream into complete frames, keeping the remainder.
 * Returns `{ frames, rest }`; malformed lines are skipped, never fatal.
 */
export function splitFrames(buffer: string): {
  frames: string[];
  rest: string;
} {
  const parts = buffer.split("\n");
  const rest = parts.pop() ?? "";
  return {
    frames: parts.filter((line) => line.trim().length > 0),
    rest,
  };
}

/** Parse one frame; throws a structured error on malformed JSON. */
export function parseFrame(line: string): unknown {
  try {
    return JSON.parse(line) as unknown;
  } catch {
    throw new Error("malformed JSON-RPC frame");
  }
}
