#!/usr/bin/env node
/**
 * Deterministic browser-use worker CLI.
 *
 * Reads one JSON action from argv[2] (or stdin), runs it through the mock
 * adapter, and prints a receipt-shaped JSON object. No network, no display.
 *
 *   node ./dist/worker.js '{"action":"snapshot_dom","url":"https://example.com"}'
 *   echo '{...}' | node ./dist/worker.js
 *   node ./dist/worker.js --live '{...}'   # opt-in live Chromium (needs playwright)
 */
import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";
import { BrowserActionRequestSchema } from "@talos/contracts";
import { MockBrowserAdapter } from "./mock.js";
import { liveNavigate } from "./live.js";

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
  switch (request.action) {
    case "navigate":
      if (options?.live === true) {
        const live = await liveNavigate(taskId, url, request.target.timeoutMs);
        if (!live.available) {
          return {
            status: "disabled",
            error: { code: "live_unavailable", message: live.reason },
            simulated: true,
          };
        }
        return { status: "success", ...live.output };
      }
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

async function readStdin(): Promise<string> {
  return readFileSync(0, "utf8");
}

async function main(): Promise<void> {
  const args = process.argv.slice(2);
  const live = args.includes("--live");
  const payloadArg = args.find((arg) => !arg.startsWith("--"));
  const raw = payloadArg ?? (await readStdin());
  let input: WorkerInput;
  try {
    input = JSON.parse(raw) as WorkerInput;
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
  const receipt = await runAction(input, { live });
  console.log(JSON.stringify(receipt, null, 2));
}

const invokedDirectly =
  process.argv[1] !== undefined &&
  import.meta.url === pathToFileURL(process.argv[1]).href;

if (invokedDirectly) {
  await main();
}
