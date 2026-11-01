/**
 * Production live Chromium adapter (Playwright).
 *
 * Disabled by default: it only activates when BOTH conditions hold:
 *  1. `TALOS_BROWSER_LIVE=1` is set, and
 *  2. the `playwright` package is installed with a downloaded Chromium.
 *
 * CI never satisfies these, so CI always exercises the mock path.
 * To enable locally: `pnpm --filter @talos/browser-use run setup:chromium`
 * (`playwright install chromium`), then run with `TALOS_BROWSER_LIVE=1`.
 *
 * Safety is identical to the mock path: selector resolution before mutation,
 * dry-run validation, high-risk approval gates, no password auto-fill,
 * forbidden-scheme blocks, sensitive-page artifact suppression, and
 * per-task isolated profiles.
 */
import { createHash } from "node:crypto";
import type { BrowserAction } from "@talos/contracts";
import {
  assertNavigableUrl,
  BrowserGuardError,
  isCredentialTarget,
  isSensitiveUrl,
} from "./guard.js";
import { countPasswordFields, storeArtifact } from "./artifacts.js";
import { LiveSessionManager, type ManagedSession } from "./sessions.js";

/** SHA-256 helper for DOM and capture hashing. */
export function sha256Text(input: string): string {
  return `sha256:${createHash("sha256").update(input, "utf8").digest("hex")}`;
}

export interface LiveResult {
  /** False when the live adapter is unavailable (disabled or no Playwright). */
  available: false;
  /** Machine-readable reason when unavailable. */
  reason: string;
  /** Receipt-shaped output when an action actually ran. */
  output?: Record<string, unknown>;
}

/** Minimal structural surface used from Playwright (avoids a hard dep). */
export interface LivePage {
  url(): string;
  title(): Promise<string>;
  goto(
    url: string,
    options?: { timeout?: number; waitUntil?: string },
  ): Promise<unknown>;
}

/** Locator surface used for DOM-first resolution and actions. */
export interface LiveLocator {
  count(): Promise<number>;
  first(): LiveLocator;
  isVisible(): Promise<boolean>;
  isEnabled(): Promise<boolean>;
  click(options?: { timeout?: number }): Promise<void>;
  fill(text: string, options?: { timeout?: number }): Promise<void>;
  press(key: string, options?: { timeout?: number }): Promise<void>;
  selectOption(
    value: string | ReadonlyArray<string>,
    options?: { timeout?: number },
  ): Promise<readonly string[]>;
  scrollIntoViewIfNeeded(options?: { timeout?: number }): Promise<void>;
  textContent(): Promise<string | null>;
  getAttribute(name: string): Promise<string | null>;
  screenshot(options?: {
    type?: string;
    quality?: number;
  }): Promise<Uint8Array>;
}

/** Rich page surface for real actions (Playwright satisfies it structurally). */
export interface RichLivePage extends LivePage {
  locator(selector: string): LiveLocator;
  evaluate<T>(script: string, arg?: unknown): Promise<T>;
  content(): Promise<string>;
  screenshot(options?: {
    type?: string;
    fullPage?: boolean;
    clip?: { x: number; y: number; width: number; height: number };
    mask?: ReadonlyArray<unknown>;
    quality?: number;
  }): Promise<Uint8Array>;
  mouse: { wheel(x: number, y: number): Promise<void> };
}

export interface LiveBrowserContext {
  pages(): RichLivePage[];
  newPage(): Promise<RichLivePage>;
  close(): Promise<void>;
}

interface PlaywrightChromium {
  launchPersistentContext(
    dir: string,
    options?: Record<string, unknown>,
  ): Promise<LiveBrowserContext>;
}

interface PlaywrightModule {
  chromium: PlaywrightChromium;
}

async function loadPlaywright(): Promise<PlaywrightModule | null> {
  try {
    // Optional peer: absent in CI by design.
    return (await import("playwright")) as PlaywrightModule;
  } catch {
    return null;
  }
}

/** True only when the operator explicitly opted into live browsing. */
export function liveEnabled(): boolean {
  return process.env.TALOS_BROWSER_LIVE === "1";
}

/** Headless unless explicitly disabled (`TALOS_BROWSER_HEADLESS=0`). */
export function liveHeadless(): boolean {
  return process.env["TALOS_BROWSER_HEADLESS"] !== "0";
}

/** Default launch options: isolated profile, no first-run noise, no extras. */
export function defaultLaunchOptions(headless = true): Record<string, unknown> {
  return {
    headless,
    acceptDownloads: false,
    args: ["--no-first-run", "--disable-default-apps", "--disable-extensions"],
  };
}

/** Resolve a live Chromium context or explain why it is unavailable. */
export async function ensureLiveContext(
  profileDir: string,
): Promise<{ available: true; context: LiveBrowserContext } | LiveResult> {
  if (!liveEnabled()) {
    return {
      available: false,
      reason: "disabled: set TALOS_BROWSER_LIVE=1 to opt into live Chromium",
    };
  }
  const playwright = await loadPlaywright();
  if (playwright === null) {
    return {
      available: false,
      reason: "unavailable: install playwright + chromium (see README)",
    };
  }
  const context = await playwright.chromium.launchPersistentContext(
    profileDir,
    defaultLaunchOptions(liveHeadless()),
  );
  return { available: true, context };
}

/** Live navigate via a persistent isolated context (opt-in only). */
export async function liveNavigate(
  profileDir: string,
  url: string,
  timeoutMs = 10000,
): Promise<
  | { available: true; reason: string; output: Record<string, unknown> }
  | LiveResult
> {
  const resolved = await ensureLiveContext(profileDir);
  if (!resolved.available) {
    return resolved;
  }
  try {
    const page =
      resolved.context.pages()[0] ?? (await resolved.context.newPage());
    await page.goto(url, { timeout: timeoutMs, waitUntil: "domcontentloaded" });
    const title = await page.title();
    return {
      available: true,
      reason: "ok",
      output: { url: page.url(), title, simulated: false },
    };
  } finally {
    await resolved.context.close();
  }
}

/** Live action input (JSON-RPC params / worker input shape). */
export interface LiveActionInput {
  action: BrowserAction;
  url?: string | undefined;
  selector?: string | undefined;
  text?: string | undefined;
  value?: string | undefined;
  direction?: "up" | "down" | "top" | "bottom" | undefined;
  pixels?: number | undefined;
  maxChars?: number | undefined;
  fullPage?: boolean | undefined;
  submit?: boolean | undefined;
  highRisk?: boolean | undefined;
  highRiskKind?: string | undefined;
  timeoutMs?: number | undefined;
  taskId: string;
  /** Explicit human approval for high-risk mutations. */
  approved?: boolean | undefined;
  /** Return a small JPEG thumbnail data URL (bounded). */
  thumbnail?: boolean | undefined;
  /** Extra origin allowlist merged with the environment list. */
  allowedOrigins?: string[] | undefined;
}

/** Structured live action result (receipt-shaped, never raw secrets). */
export interface LiveActionResult {
  status: "success" | "failed" | "denied";
  selector?: string | undefined;
  resolved?:
    | {
        selector: string;
        found: boolean;
        visible: boolean;
        interactable: boolean;
        matchCount: number;
        domHash: string;
      }
    | undefined;
  domHash?: string | undefined;
  title?: string | undefined;
  targetUrl?: string | undefined;
  screenshotHash?: string | undefined;
  screenshotPath?: string | undefined;
  /** Bounded JPEG data URL when `thumbnail` was requested. */
  thumbnailDataUrl?: string | undefined;
  extractedText?: string | undefined;
  extractedTextHash?: string | undefined;
  truncated?: boolean | undefined;
  sensitive: boolean;
  redacted: boolean;
  latencyMs: number;
  error?: { code: string; message: string } | undefined;
  simulated: false;
}

/** Live adapter options. */
export interface LiveAdapterOptions {
  /** Context launcher (real Playwright by default, fakes in tests). */
  launch?: ((profileDir: string) => Promise<LiveBrowserContext>) | undefined;
  /** Max warm sessions (default 1). */
  maxSessions?: number | undefined;
  /** Idle auto-close in ms (default 5 minutes). */
  idleTimeoutMs?: number | undefined;
  /** Profile root for isolated sessions. */
  profileRoot?: string | undefined;
  /** Extra sensitive domains merged with the environment list. */
  sensitiveDomains?: string[] | undefined;
  /** Extra allowed origins merged with the environment list. */
  allowedOrigins?: string[] | undefined;
  /** Clock for tests. */
  now?: (() => number) | undefined;
}

/** Production live Chromium adapter: DOM-first, policy-shaped receipts. */
export class LiveBrowserAdapter {
  readonly sessions: LiveSessionManager;
  private readonly profileRoot?: string | undefined;
  private readonly sensitiveDomains: string[];
  private readonly allowedOrigins: string[];

  constructor(options?: LiveAdapterOptions | undefined) {
    const launch =
      options?.launch ??
      (async (profileDir: string): Promise<LiveBrowserContext> => {
        const playwright = await loadPlaywright();
        if (playwright === null) {
          throw new Error("playwright unavailable");
        }
        return playwright.chromium.launchPersistentContext(
          profileDir,
          defaultLaunchOptions(liveHeadless()),
        );
      });
    this.sessions = new LiveSessionManager({
      maxSessions: options?.maxSessions,
      idleTimeoutMs: options?.idleTimeoutMs,
      launch,
      now: options?.now,
    });
    this.profileRoot = options?.profileRoot;
    this.sensitiveDomains = options?.sensitiveDomains ?? [];
    this.allowedOrigins = options?.allowedOrigins ?? [];
  }

  /** Active warm session count. */
  sessionCount(): number {
    return this.sessions.count();
  }

  /** Close one task session (idempotent). */
  async closeSession(taskId: string): Promise<boolean> {
    return this.sessions.closeSession(taskId);
  }

  /** Close all warm sessions (emergency-stop path). Returns closed ids. */
  async closeAllSessions(): Promise<string[]> {
    return this.sessions.closeAll();
  }

  /** Execute one guarded action against the task's warm session. */
  async executeAction(input: LiveActionInput): Promise<LiveActionResult> {
    const started = Date.now();
    const timeoutMs = clampTimeout(input.timeoutMs);
    const origins = [...this.allowedOrigins, ...(input.allowedOrigins ?? [])];
    try {
      if (input.action === "navigate") {
        if (input.url === undefined) {
          return this.failed(
            input,
            started,
            "invalid_params",
            "navigate needs url",
          );
        }
        assertNavigableUrl(input.url, origins.length > 0 ? origins : undefined);
      } else if (input.url !== undefined) {
        assertNavigableUrl(input.url, origins.length > 0 ? origins : undefined);
      }
      const session = await this.sessions.launchSession(
        input.taskId,
        this.profileRoot,
      );
      const page =
        session.context.pages()[0] ?? (await session.context.newPage());
      await this.ensureUrl(page, input, timeoutMs);
      const result = await this.dispatch(
        page,
        session,
        input,
        timeoutMs,
        started,
      );
      this.sessions.touch(input.taskId, result.targetUrl, result.title);
      return result;
    } catch (error) {
      if (error instanceof BrowserGuardError) {
        return this.failed(input, started, error.code, error.message);
      }
      if (error instanceof Error) {
        console.error(`[browser] action failed: ${error.name}`);
      }
      const mapped = playError("browser action failed", error);
      return this.failed(input, started, mapped.code, mapped.message);
    }
  }

  private failed(
    input: LiveActionInput,
    started: number,
    code: string,
    message: string,
  ): LiveActionResult {
    return {
      status: "failed",
      targetUrl: input.url,
      sensitive: input.url !== undefined && this.isSensitive(input.url),
      redacted: false,
      latencyMs: Date.now() - started,
      error: { code, message },
      simulated: false,
    };
  }

  private isSensitive(url: string | undefined): boolean {
    if (url === undefined) {
      return false;
    }
    return isSensitiveUrl(url, this.sensitiveDomains);
  }

  /**
   * Ensure the page is on the action's target URL. Single-shot calls and
   * fresh sessions start blank; navigating first keeps every action honest
   * about what it observes (and keeps domHash/content truthful).
   */
  private async ensureUrl(
    page: RichLivePage,
    input: LiveActionInput,
    timeoutMs: number,
  ): Promise<void> {
    if (input.url === undefined || input.action === "navigate") {
      return;
    }
    const current = page.url();
    if (
      current !== "about:blank" &&
      stripTrailingSlash(current) === stripTrailingSlash(input.url)
    ) {
      return;
    }
    try {
      await page.goto(input.url, {
        timeout: timeoutMs,
        waitUntil: "domcontentloaded",
      });
    } catch (error) {
      const name = error instanceof Error ? error.name : "Error";
      throw new Error(
        `navigation failed: ${/timeout/i.test(name) ? "timed out" : name}`,
      );
    }
  }

  private async dispatch(
    page: RichLivePage,
    session: ManagedSession,
    input: LiveActionInput,
    timeoutMs: number,
    started: number,
  ): Promise<LiveActionResult> {
    switch (input.action) {
      case "navigate":
        return this.doNavigate(page, input, timeoutMs, started);
      case "snapshot_dom":
        return this.doSnapshot(page, input, timeoutMs, started);
      case "extract_text":
        return this.doExtract(page, input, timeoutMs, started);
      case "screenshot":
        return this.doScreenshot(page, session, input, timeoutMs, started);
      case "scroll":
        return this.doScroll(page, input, timeoutMs, started);
      case "click":
      case "type":
      case "select":
        return this.doMutate(page, input, timeoutMs, started);
    }
  }

  private async pageIdentity(page: RichLivePage): Promise<{
    url: string;
    title: string;
    domHash: string;
    html: string;
  }> {
    const html = await page.content();
    const url = page.url();
    const title = await page.title().catch(() => "");
    return { url, title, domHash: sha256Text(html), html };
  }

  private async doNavigate(
    page: RichLivePage,
    input: LiveActionInput,
    timeoutMs: number,
    started: number,
  ): Promise<LiveActionResult> {
    const url = input.url ?? "";
    try {
      await page.goto(url, {
        timeout: timeoutMs,
        waitUntil: "domcontentloaded",
      });
    } catch (error) {
      return {
        status: "failed",
        targetUrl: url,
        sensitive: this.isSensitive(url),
        redacted: false,
        latencyMs: Date.now() - started,
        error: playError("navigation failed", error),
        simulated: false,
      };
    }
    const identity = await this.pageIdentity(page);
    return {
      status: "success",
      targetUrl: identity.url,
      title: identity.title,
      domHash: identity.domHash,
      sensitive: this.isSensitive(identity.url),
      redacted: false,
      latencyMs: Date.now() - started,
      simulated: false,
    };
  }

  private async resolve(
    page: RichLivePage,
    selector: string,
    scopeUrl: string,
  ): Promise<NonNullable<LiveActionResult["resolved"]>> {
    const locator = page.locator(selector);
    let count = 0;
    try {
      count = await locator.count();
    } catch {
      count = 0;
    }
    const found = count > 0;
    let visible = false;
    let interactable = false;
    if (found) {
      try {
        visible = await locator.first().isVisible();
        interactable = visible && (await locator.first().isEnabled());
      } catch {
        visible = false;
        interactable = false;
      }
    }
    return {
      selector,
      found,
      visible,
      interactable,
      matchCount: count,
      domHash: sha256Text(scopeUrl),
    };
  }

  private async doSnapshot(
    page: RichLivePage,
    _input: LiveActionInput,
    timeoutMs: number,
    started: number,
  ): Promise<LiveActionResult> {
    void timeoutMs;
    const identity = await this.pageIdentity(page);
    return {
      status: "success",
      targetUrl: identity.url,
      title: identity.title,
      domHash: identity.domHash,
      sensitive: this.isSensitive(identity.url),
      redacted: false,
      latencyMs: Date.now() - started,
      simulated: false,
    };
  }

  private async doExtract(
    page: RichLivePage,
    input: LiveActionInput,
    timeoutMs: number,
    started: number,
  ): Promise<LiveActionResult> {
    void timeoutMs;
    const url = input.url ?? page.url();
    let text: string;
    try {
      if (input.selector !== undefined) {
        const locator = page.locator(input.selector);
        text = (await locator.first().textContent()) ?? "";
      } else {
        text =
          (await page.evaluate<string | null>(
            "document && document.body ? document.body.innerText : ''",
          )) ?? "";
      }
    } catch (error) {
      return {
        status: "failed",
        targetUrl: url,
        sensitive: this.isSensitive(url),
        redacted: false,
        latencyMs: Date.now() - started,
        error: playError("extract failed", error),
        simulated: false,
      };
    }
    const maxChars = Math.min(input.maxChars ?? 5000, 20000);
    const truncated = text.length > maxChars;
    const capped = truncated ? text.slice(0, maxChars) : text;
    const identity = await this.pageIdentity(page);
    return {
      status: "success",
      targetUrl: identity.url,
      title: identity.title,
      domHash: identity.domHash,
      extractedText: capped,
      extractedTextHash: sha256Text(text),
      truncated,
      sensitive: this.isSensitive(identity.url),
      redacted: false,
      latencyMs: Date.now() - started,
      simulated: false,
    };
  }

  private async doScreenshot(
    page: RichLivePage,
    session: ManagedSession,
    input: LiveActionInput,
    timeoutMs: number,
    started: number,
  ): Promise<LiveActionResult> {
    void timeoutMs;
    const pre = await this.pageIdentity(page).catch(() => null);
    const url = pre?.url ?? input.url ?? page.url();
    const sensitive = this.isSensitive(url);
    const redacted = pre !== null && countPasswordFields(pre.html) > 0;
    const mask = redacted ? await this.passwordMasks(page) : undefined;
    let buffer: Uint8Array;
    try {
      buffer = await page.screenshot({
        type: "png",
        fullPage: input.fullPage ?? false,
        ...(mask === undefined ? {} : { mask }),
      });
    } catch (error) {
      return {
        status: "failed",
        targetUrl: url,
        sensitive,
        redacted,
        latencyMs: Date.now() - started,
        error: playError("screenshot failed", error),
        simulated: false,
      };
    }
    const artifact = await storeArtifact(
      session.session.profileDir,
      buffer,
      "png",
      { sensitive },
    );
    let thumbnailDataUrl: string | undefined;
    if (input.thumbnail === true && !sensitive) {
      thumbnailDataUrl = await this.thumbnail(page);
    }
    const identity = await this.pageIdentity(page);
    return {
      status: "success",
      targetUrl: identity.url,
      title: identity.title,
      domHash: identity.domHash,
      screenshotHash: artifact.hash,
      screenshotPath: artifact.path,
      thumbnailDataUrl,
      sensitive,
      redacted,
      latencyMs: Date.now() - started,
      simulated: false,
    };
  }

  private async thumbnail(page: RichLivePage): Promise<string | undefined> {
    try {
      const bytes = await page.screenshot({
        type: "jpeg",
        quality: 40,
        clip: { x: 0, y: 0, width: 480, height: 270 },
        fullPage: false,
      });
      return `data:image/jpeg;base64,${Buffer.from(bytes).toString("base64")}`;
    } catch {
      return undefined;
    }
  }

  private async passwordMasks(
    page: RichLivePage,
  ): Promise<unknown[] | undefined> {
    try {
      const count = await page
        .locator("input[type=password]")
        .count()
        .catch(() => 0);
      if (count === 0) {
        return undefined;
      }
      return [page.locator("input[type=password]") as unknown];
    } catch {
      return undefined;
    }
  }

  private async doScroll(
    page: RichLivePage,
    input: LiveActionInput,
    timeoutMs: number,
    started: number,
  ): Promise<LiveActionResult> {
    void timeoutMs;
    const direction = input.direction ?? "down";
    const pixels = Math.min(Math.max(input.pixels ?? 800, 1), 10000);
    try {
      if (direction === "top") {
        await page.evaluate("window.scrollTo(0, 0)");
      } else if (direction === "bottom") {
        await page.evaluate(
          "window.scrollTo(0, document.body ? document.body.scrollHeight : 0)",
        );
      } else if (direction === "up") {
        await page.mouse.wheel(0, -pixels);
      } else {
        await page.mouse.wheel(0, pixels);
      }
    } catch (error) {
      return {
        status: "failed",
        sensitive: this.isSensitive(page.url()),
        redacted: false,
        latencyMs: Date.now() - started,
        error: playError("scroll failed", error),
        simulated: false,
      };
    }
    const identity = await this.pageIdentity(page);
    return {
      status: "success",
      targetUrl: identity.url,
      title: identity.title,
      domHash: identity.domHash,
      sensitive: this.isSensitive(identity.url),
      redacted: false,
      latencyMs: Date.now() - started,
      simulated: false,
    };
  }

  private async doMutate(
    page: RichLivePage,
    input: LiveActionInput,
    timeoutMs: number,
    started: number,
  ): Promise<LiveActionResult> {
    const selector = input.selector ?? "";
    const url = input.url ?? page.url();
    const resolved = await this.resolve(page, selector, url);
    if (!resolved.found) {
      return this.actionFailed(
        input,
        resolved,
        started,
        "not_found",
        `${input.action}: selector not found`,
      );
    }
    if (!resolved.visible) {
      return this.actionFailed(
        input,
        resolved,
        started,
        "not_visible",
        `${input.action}: element not visible`,
      );
    }
    if (!resolved.interactable) {
      return this.actionFailed(
        input,
        resolved,
        started,
        "not_interactable",
        `${input.action}: element not interactable`,
      );
    }
    if (input.highRisk === true && input.approved !== true) {
      return {
        ...this.deniedBase(input, resolved, started),
        status: "denied",
        error: {
          code: "approval_required",
          message: `${input.action}: high-risk mutation needs explicit approval`,
        },
      };
    }
    if (input.action === "type" && isCredentialTarget(selector)) {
      return {
        ...this.deniedBase(input, resolved, started),
        status: "denied",
        error: {
          code: "auth_required",
          message:
            "password fields are never auto-filled; human handling required",
        },
      };
    }
    try {
      const locator = page.locator(selector).first();
      if (input.action === "click") {
        await locator.click({ timeout: timeoutMs });
      } else if (input.action === "type") {
        await locator.fill(input.text ?? "", { timeout: timeoutMs });
        if (input.submit === true) {
          await locator.press("Enter", { timeout: timeoutMs });
        }
      } else {
        await locator.selectOption(input.value ?? "", { timeout: timeoutMs });
      }
    } catch (error) {
      return {
        ...this.deniedBase(input, resolved, started),
        status: "failed",
        error: playError(`${input.action} failed`, error),
      };
    }
    const identity = await this.pageIdentity(page);
    return {
      status: "success",
      selector,
      resolved,
      targetUrl: identity.url,
      title: identity.title,
      domHash: identity.domHash,
      sensitive: this.isSensitive(identity.url),
      redacted: false,
      latencyMs: Date.now() - started,
      simulated: false,
    };
  }

  private deniedBase(
    input: LiveActionInput,
    resolved: NonNullable<LiveActionResult["resolved"]>,
    started: number,
  ): LiveActionResult {
    return {
      status: "denied",
      selector: input.selector,
      resolved,
      targetUrl: input.url,
      domHash: resolved.domHash,
      sensitive: this.isSensitive(input.url),
      redacted: false,
      latencyMs: Date.now() - started,
      simulated: false,
    };
  }

  private actionFailed(
    input: LiveActionInput,
    resolved: NonNullable<LiveActionResult["resolved"]>,
    started: number,
    code: string,
    message: string,
  ): LiveActionResult {
    return {
      ...this.deniedBase(input, resolved, started),
      status: "failed",
      error: { code, message },
    };
  }
}

function clampTimeout(timeoutMs: number | undefined): number {
  if (timeoutMs === undefined) {
    return 10000;
  }
  return Math.min(Math.max(Math.floor(timeoutMs), 100), 60000);
}

/** Trailing-slash-insensitive URL comparison for session reuse. */
function stripTrailingSlash(url: string): string {
  return url.endsWith("/") && url.length > 1 ? url.slice(0, -1) : url;
}

/** Map Playwright failures to codes without leaking page content. */
function playError(
  prefix: string,
  error: unknown,
): { code: string; message: string } {
  const name = error instanceof Error ? error.name : "Error";
  if (/timeout/i.test(name)) {
    return { code: "timeout", message: `${prefix}: timed out` };
  }
  return { code: "action_failed", message: `${prefix}: ${name}` };
}
