/**
 * Optional live Chromium adapter (Playwright).
 *
 * Disabled by default: it only activates when BOTH conditions hold:
 *  1. `TALOS_BROWSER_LIVE=1` is set, and
 *  2. the `playwright` package is installed with a downloaded Chromium.
 *
 * CI never satisfies these, so CI always exercises the mock path.
 * To enable locally: `pnpm add -D playwright && npx playwright install
 * chromium`, then run with `TALOS_BROWSER_LIVE=1`.
 */

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

export interface LiveBrowserContext {
  pages(): LivePage[];
  newPage(): Promise<LivePage>;
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
    {
      headless: true,
      acceptDownloads: false,
    },
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
