#!/usr/bin/env node
/**
 * Deterministic browser-use worker CLI + JSON-RPC serve mode.
 *
 * Single-shot (backwards compatible):
 *   node ./dist/worker.js '{"action":"snapshot_dom","url":"https://example.com"}'
 *   echo '{...}' | node ./dist/worker.js
 *   node ./dist/worker.js --live '{...}'   # opt-in live Chromium (needs playwright)
 *
 * Serve mode (persistent worker, line-delimited JSON-RPC on stdio):
 *   node ./dist/worker.js --serve
 *   {"id":"1","method":"snapshot_dom","params":{"url":"https://example.com","taskId":"t"}}
 *   {"id":"2","method":"session.status","params":{}}
 *
 * Methods: the 8 browser actions plus `session.launch|status|close|close_all`
 * and `ping`. Async `{ event, payload }` frames announce auth-required,
 * session-closed, and crash conditions. Every call races a 30s worker
 * timeout; high-risk mutations require `approved: true`.
 */
import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";
import { BrowserActionRequestSchema } from "@talos/contracts";
import { MockBrowserAdapter } from "./mock.js";
import {
  encodeFrame,
  parseFrame,
  RpcRequestSchema,
  splitFrames,
  WorkerEventName,
  type RpcRequest,
} from "./protocol.js";
import {
  liveEnabled,
  LiveBrowserAdapter,
  type LiveActionInput,
} from "./live.js";

export interface WorkerInput {
  action: string;
  url?: string;
  selector?: string;
  text?: string;
  value?: string;
  direction?: string;
  pixels?: number;
  maxChars?: number;
  fullPage?: boolean;
  submit?: boolean;
  highRisk?: boolean;
  highRiskKind?: string;
  timeoutMs?: number;
  taskId?: string;
  /** Explicit human approval for high-risk live mutations. */
  approved?: boolean | undefined;
  /** Return a bounded JPEG thumbnail data URL (screenshots). */
  thumbnail?: boolean | undefined;
  /** Extra origin allowlist merged with the environment list. */
  allowedOrigins?: string[] | undefined;
}

/** Per-call worker timeout (the parent kills us past this). */
export const WORKER_CALL_TIMEOUT_MS = 30_000;

function withTimeout<T>(
  promise: Promise<T>,
  ms = WORKER_CALL_TIMEOUT_MS,
): Promise<T> {
  let timer: NodeJS.Timeout | undefined;
  const timeout = new Promise<never>((_, reject) => {
    timer = setTimeout(() => {
      reject(new Error("worker call timed out"));
    }, ms);
    timer.unref?.();
  });
  return Promise.race([promise, timeout]).finally(() => {
    if (timer !== undefined) {
      clearTimeout(timer);
    }
  });
}

export async function runAction(
  input: WorkerInput,
  options?: { live?: boolean; root?: string; taskId?: string },
): Promise<Record<string, unknown>> {
  const taskId = options?.taskId ?? input.taskId ?? "task-worker-1";
  const parsed = BrowserActionRequestSchema.safeParse({
    actionId: `act-${Date.now()}`,
    taskId,
    action: input.action,
    target: {
      url: input.url,
      selector: input.selector,
      timeoutMs: input.timeoutMs,
    },
    text: input.text,
    value: input.value,
    direction: input.direction,
    pixels: input.pixels,
    maxChars: input.maxChars,
    fullPage: input.fullPage,
    highRisk: input.highRisk,
    highRiskKind: input.highRiskKind,
    dryRun: !options?.live,
  });
  if (!parsed.success) {
    return {
      status: "failed",
      error: { code: "invalid_params", message: parsed.error.message },
      simulated: true,
    };
  }
  const request = parsed.data;
  const mock = new MockBrowserAdapter();
  const url = request.target.url ?? "(current page)";
  const liveInput: LiveActionInput = {
    action: request.action,
    url: request.target.url,
    selector: request.target.selector,
    text: request.text,
    value: request.value,
    direction: request.direction,
    pixels: request.pixels,
    maxChars: request.maxChars,
    fullPage: request.fullPage,
    submit: undefined,
    highRisk: request.highRisk,
    highRiskKind: request.highRiskKind,
    timeoutMs: request.target.timeoutMs,
    taskId,
    approved: input.approved,
    thumbnail: input.thumbnail,
    allowedOrigins: input.allowedOrigins,
  };
  if (options?.live === true) {
    return runLiveAction(liveInput, { submit: request.submit });
  }
  switch (request.action) {
    case "navigate":
      return { status: "success", ...mock.planNavigate(url) };
    case "snapshot_dom":
      return {
        status: "success",
        ...mock.snapshot(url, request.target.selector),
      };
    case "extract_text":
      return {
        status: "success",
        ...mock.extract(url, request.target.selector, request.maxChars ?? 5000),
      };
    case "screenshot":
      return {
        status: "success",
        ...mock.screenshot(url, request.fullPage ?? false),
      };
    case "scroll":
      return {
        status: "success",
        ...mock.planScroll(
          url === "(current page)" ? undefined : url,
          request.direction ?? "down",
          request.pixels ?? 800,
        ),
      };
    case "click":
      return {
        status: "success",
        ...mock.planClick(
          url === "(current page)" ? undefined : url,
          request.target.selector ?? "",
          {
            highRisk: request.highRisk,
            highRiskKind: request.highRiskKind,
          },
        ),
      };
    case "type":
      return {
        status: "success",
        ...mock.planType(
          url === "(current page)" ? undefined : url,
          request.target.selector ?? "",
          request.text ?? "",
          {
            submit: request.submit,
          },
        ),
      };
    case "select":
      return {
        status: "success",
        ...mock.planSelect(
          url === "(current page)" ? undefined : url,
          request.target.selector ?? "",
          request.value,
        ),
      };
  }
}

async function runLiveAction(
  input: LiveActionInput,
  extra?: { submit?: boolean | undefined },
): Promise<Record<string, unknown>> {
  const adapter = new LiveBrowserAdapter();
  try {
    const result = await adapter.executeAction({
      ...input,
      submit: extra?.submit,
    });
    await adapter.closeAllSessions().catch(() => undefined);
    return result as unknown as Record<string, unknown>;
  } catch (error) {
    await adapter.closeAllSessions().catch(() => undefined);
    return {
      status: "failed",
      error: {
        code: "live_unavailable",
        message:
          error instanceof Error ? error.message : "live execution failed",
      },
      simulated: true,
    };
  }
}

/** Serve-mode dispatcher (injectable adapter for hermetic tests). */
export class WorkerServer {
  private readonly adapter: LiveBrowserAdapter;

  constructor(adapter?: LiveBrowserAdapter | undefined) {
    this.adapter = adapter ?? new LiveBrowserAdapter();
  }

  /** Active warm session count (max 1 by default). */
  sessionCount(): number {
    return this.adapter.sessionCount();
  }

  async handle(raw: unknown): Promise<{
    response: {
      id: string;
      ok: boolean;
      result?: unknown;
      error?: { code: string; message: string };
    };
    events: Array<{ event: string; payload: unknown }>;
  }> {
    const parsed = RpcRequestSchema.safeParse(raw);
    if (!parsed.success) {
      return {
        response: {
          id: "unknown",
          ok: false,
          error: {
            code: "invalid_params",
            message: "malformed JSON-RPC frame",
          },
        },
        events: [],
      };
    }
    const request: RpcRequest = parsed.data;
    try {
      const { result, events } = await withTimeout(this.dispatch(request));
      return { response: { id: request.id, ok: true, result }, events };
    } catch (error) {
      const message =
        error instanceof Error ? error.message : "worker call failed";
      const code =
        message === "worker call timed out"
          ? "worker_timeout"
          : (error as { code?: unknown }).code === "invalid_params"
            ? "invalid_params"
            : "worker_failed";
      return {
        response: { id: request.id, ok: false, error: { code, message } },
        events:
          code === "worker_timeout"
            ? [
                {
                  event: WorkerEventName.browserWorkerCrashed,
                  payload: { id: request.id },
                },
              ]
            : [],
      };
    }
  }

  private async dispatch(request: RpcRequest): Promise<{
    result: unknown;
    events: Array<{ event: string; payload: unknown }>;
  }> {
    const params = (request.params ?? {}) as Record<string, unknown>;
    const taskId =
      typeof params["taskId"] === "string" && params["taskId"].length > 0
        ? (params["taskId"] as string)
        : "task-worker-1";
    switch (request.method) {
      case "ping":
        return { result: { ok: true }, events: [] };
      case "session.launch": {
        const persistent = params["persistent"] === true;
        const session = await this.adapter.sessions.launchSession(taskId);
        void persistent;
        return {
          result: {
            sessionId: session.session.id,
            taskId,
            profileDir: session.session.profileDir,
          },
          events: [],
        };
      }
      case "session.status": {
        const one =
          typeof params["taskId"] === "string"
            ? this.adapter.sessions.get(params["taskId"] as string)
            : undefined;
        const list = one !== undefined ? [one] : [];
        return {
          result: list.map((session) => ({
            taskId: session.taskId,
            currentUrl: session.currentUrl ?? null,
            title: session.title ?? null,
            lastActivityMs: session.lastActivityMs,
          })),
          events: [],
        };
      }
      case "session.close": {
        const closed = await this.adapter.sessions.closeSession(taskId);
        return {
          result: { closed },
          events: closed
            ? [
                {
                  event: WorkerEventName.browserSessionClosed,
                  payload: { taskId },
                },
              ]
            : [],
        };
      }
      case "session.close_all": {
        const closed = await this.adapter.closeAllSessions();
        return { result: { closed }, events: [] };
      }
      default: {
        // Browser actions arrive as `click` or namespaced `browser.click`.
        const action = request.method.startsWith("browser.")
          ? request.method.slice("browser.".length)
          : request.method;
        const receipt = await runLiveMethod(
          this.adapter,
          taskId,
          action,
          params,
        );
        const events: Array<{ event: string; payload: unknown }> = [];
        const error = (receipt as { error?: { code?: string } | undefined })
          .error;
        if (error?.code === "auth_required") {
          events.push({
            event: WorkerEventName.browserAuthRequired,
            payload: { taskId, action },
          });
        }
        return { result: receipt, events };
      }
    }
  }

  /** Close all sessions (SIGTERM/SIGINT path). */
  async shutdown(): Promise<string[]> {
    return this.adapter.closeAllSessions();
  }
}

async function runLiveMethod(
  adapter: LiveBrowserAdapter,
  taskId: string,
  action: string,
  params: Record<string, unknown>,
): Promise<Record<string, unknown>> {
  const str = (value: unknown): string | undefined =>
    typeof value === "string" && value.length > 0 ? value : undefined;
  const num = (value: unknown): number | undefined =>
    typeof value === "number" && Number.isFinite(value) ? value : undefined;
  const bool = (value: unknown): boolean | undefined =>
    typeof value === "boolean" ? value : undefined;
  const strings = (value: unknown): string[] | undefined =>
    Array.isArray(value) && value.every((entry) => typeof entry === "string")
      ? (value as string[])
      : undefined;
  const input: LiveActionInput = {
    action: action as LiveActionInput["action"],
    url: str(params["url"]),
    selector: str(params["selector"]),
    text: str(params["text"]),
    value: str(params["value"]),
    direction: ["up", "down", "top", "bottom"].includes(
      String(params["direction"] ?? ""),
    )
      ? (params["direction"] as LiveActionInput["direction"])
      : undefined,
    pixels: num(params["pixels"]),
    maxChars: num(params["maxChars"]),
    fullPage: bool(params["fullPage"]),
    submit: bool(params["submit"]),
    highRisk: bool(params["highRisk"]),
    highRiskKind: str(params["highRiskKind"]),
    timeoutMs: num(params["timeoutMs"]),
    taskId,
    approved: bool(params["approved"]),
    thumbnail: bool(params["thumbnail"]),
    allowedOrigins: strings(params["allowedOrigins"]),
  };
  if (
    ![
      "navigate",
      "click",
      "type",
      "scroll",
      "select",
      "extract_text",
      "screenshot",
      "snapshot_dom",
    ].includes(action)
  ) {
    const error = new Error(`unknown method ${action}`);
    (error as { code?: string }).code = "invalid_params";
    throw error;
  }
  if (!liveEnabled()) {
    return {
      status: "disabled",
      error: {
        code: "live_unavailable",
        message: "set TALOS_BROWSER_LIVE=1 to opt into live Chromium",
      },
      simulated: true,
    };
  }
  const result = await adapter.executeAction(input);
  return { ...result };
}

async function readStdin(): Promise<string> {
  return readFileSync(0, "utf8");
}

async function main(): Promise<void> {
  const args = process.argv.slice(2);
  if (args.includes("--serve")) {
    await serve();
    return;
  }
  const live = args.includes("--live");
  const payloadArg = args.find((arg) => !arg.startsWith("--"));
  const raw = payloadArg ?? (await readStdin());
  let input: WorkerInput & { method?: unknown };
  try {
    input = JSON.parse(raw) as WorkerInput & { method?: unknown };
  } catch {
    console.log(
      JSON.stringify({
        status: "failed",
        error: { code: "invalid_params", message: "stdin/argv must be JSON" },
      }),
    );
    process.exitCode = 1;
    return;
  }
  if (typeof input.method === "string") {
    // Single-shot JSON-RPC frame (session.* and ping without a daemon).
    const server = new WorkerServer();
    const { response, events } = await server.handle({
      id: "single-shot",
      method: input.method,
      params: input,
    });
    for (const event of events) {
      console.error(JSON.stringify(event));
    }
    if (response.ok) {
      console.log(JSON.stringify(response.result, null, 2));
    } else {
      console.log(JSON.stringify({ status: "failed", error: response.error }));
      process.exitCode = 1;
    }
    return;
  }
  const receipt = await runAction(input, { live });
  console.log(JSON.stringify(receipt, null, 2));
}

/** NDJSON serve loop: requests in, responses + events out. */
async function serve(): Promise<void> {
  const server = new WorkerServer();
  let buffer = "";
  const onShutdown = (): void => {
    void server.shutdown().finally(() => process.exit(0));
  };
  process.once("SIGTERM", onShutdown);
  process.once("SIGINT", onShutdown);
  process.stdin.setEncoding("utf8");
  for await (const chunk of process.stdin) {
    buffer += String(chunk);
    const { frames, rest } = splitFrames(buffer);
    buffer = rest;
    for (const line of frames) {
      let raw: unknown;
      try {
        raw = parseFrame(line);
      } catch {
        process.stdout.write(
          encodeFrame({
            id: "unknown",
            ok: false,
            error: {
              code: "invalid_params",
              message: "malformed JSON-RPC frame",
            },
          }),
        );
        continue;
      }
      const { response, events } = await server.handle(raw);
      for (const event of events) {
        process.stdout.write(encodeFrame(event));
      }
      process.stdout.write(encodeFrame(response));
    }
  }
}

const invokedDirectly =
  process.argv[1] !== undefined &&
  import.meta.url === pathToFileURL(process.argv[1]).href;

if (invokedDirectly) {
  await main().catch((error: unknown) => {
    process.stdout.write(
      encodeFrame({
        event: WorkerEventName.browserWorkerCrashed,
        payload: {
          message: error instanceof Error ? error.message : "worker crashed",
        },
      }),
    );
    process.exitCode = 1;
  });
}
