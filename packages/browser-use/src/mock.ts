import {
  assertActionable,
  domHash,
  mockDom,
  resolveSelector,
  sha256Hex,
} from "./dom.js";
import { closeSession, createSession } from "./session.js";

/** Deterministic mock browser: seeded fake DOM, no network, no side effects. */
export class MockBrowserAdapter {
  /** Plan navigation (dry-run): resolve + validate, mutate nothing. */
  planNavigate(url: string, timeoutMs = 10000): Record<string, unknown> {
    const dom = mockDom(url);
    return {
      url,
      timeoutMs,
      domHash: domHash(dom),
      planned: true,
      simulated: true,
    };
  }

  /** Live navigate (mock): ensure the isolated profile, return the DOM hash. */
  async liveNavigate(
    taskId: string,
    url: string,
    root?: string | undefined,
  ): Promise<Record<string, unknown>> {
    const session = await createSession(taskId, { root });
    const dom = mockDom(url);
    return {
      url,
      domHash: domHash(dom),
      profile: session.profileDir,
      simulated: true,
    };
  }

  /** Plan a click (dry-run): resolve the selector, validate, mutate nothing. */
  planClick(
    url: string | undefined,
    selector: string,
    options?: {
      timeoutMs?: number | undefined;
      highRisk?: boolean | undefined;
      highRiskKind?: string | undefined;
    },
  ): Record<string, unknown> {
    const context = url ?? "(current page)";
    const resolved = resolveSelector(context, selector);
    assertActionable(resolved, "browser.click");
    return {
      selector,
      resolved,
      domHash: resolved.domHash,
      timeoutMs: options?.timeoutMs ?? 10000,
      highRisk: options?.highRisk ?? false,
      highRiskKind: options?.highRiskKind,
      planned: true,
      simulated: true,
    };
  }

  /** Live click (mock): profile + resolution + deterministic record. */
  async liveClick(
    taskId: string,
    url: string | undefined,
    selector: string,
    root?: string | undefined,
  ): Promise<Record<string, unknown>> {
    const session = await createSession(taskId, { root });
    const planned = this.planClick(url, selector);
    return { ...planned, planned: undefined, profile: session.profileDir };
  }

  /** Plan typing (dry-run): resolve, validate, mutate nothing. */
  planType(
    url: string | undefined,
    selector: string,
    text: string,
    options?: {
      submit?: boolean | undefined;
      timeoutMs?: number | undefined;
      highRisk?: boolean | undefined;
      highRiskKind?: string | undefined;
    },
  ): Record<string, unknown> {
    const context = url ?? "(current page)";
    const resolved = resolveSelector(context, selector);
    assertActionable(resolved, "browser.type");
    return {
      selector,
      chars: text.length,
      submit: options?.submit ?? false,
      resolved,
      domHash: resolved.domHash,
      timeoutMs: options?.timeoutMs ?? 10000,
      highRisk: options?.highRisk ?? false,
      highRiskKind: options?.highRiskKind,
      planned: true,
      simulated: true,
    };
  }

  /** Live type (mock). */
  async liveType(
    taskId: string,
    url: string | undefined,
    selector: string,
    text: string,
    root?: string | undefined,
  ): Promise<Record<string, unknown>> {
    const session = await createSession(taskId, { root });
    const planned = this.planType(url, selector, text);
    return { ...planned, planned: undefined, profile: session.profileDir };
  }

  /** Plan scrolling (dry-run). */
  planScroll(
    url: string | undefined,
    direction = "down",
    pixels = 800,
  ): Record<string, unknown> {
    const context = url ?? "(current page)";
    return {
      direction,
      pixels,
      domHash: domHash(mockDom(context)),
      planned: true,
      simulated: true,
    };
  }

  /** Plan selection (dry-run): resolve, validate, mutate nothing. */
  planSelect(
    url: string | undefined,
    selector: string,
    value?: string,
  ): Record<string, unknown> {
    const context = url ?? "(current page)";
    const resolved = resolveSelector(context, selector);
    assertActionable(resolved, "browser.select");
    return {
      selector,
      value,
      resolved,
      domHash: resolved.domHash,
      planned: true,
      simulated: true,
    };
  }

  /** Extract visible text (mock, bounded). */
  extract(
    url: string,
    selector?: string,
    maxChars = 5000,
  ): Record<string, unknown> {
    const text =
      `Mock extract of ${url} [${selector ?? "document"}]: ` +
      "header, main content, navigation and form labels.";
    const capped = text.slice(0, maxChars);
    return {
      url,
      text: capped,
      textHash: sha256Hex(text),
      domHash: domHash(mockDom(url, selector)),
      truncated: text.length > maxChars,
      simulated: true,
    };
  }

  /** Screenshot hash (mock: deterministic, no pixels stored). */
  screenshot(url: string, fullPage = false): Record<string, unknown> {
    return {
      url,
      screenshotHash: sha256Hex(
        `screenshot:${url}:full=${fullPage}:viewport=1440x900`,
      ),
      domHash: domHash(mockDom(url)),
      fullPage,
      simulated: true,
    };
  }

  /** DOM snapshot (mock). */
  snapshot(url: string, selector?: string): Record<string, unknown> {
    const dom = mockDom(url, selector);
    return {
      url,
      dom: dom,
      domHash: domHash(dom),
      simulated: true,
    };
  }

  /** Close the task session (removes the profile unless persistent). */
  async close(taskId: string, root?: string): Promise<void> {
    await closeSession({
      id: `session-${taskId}`,
      taskId,
      profileDir: `${root ?? "browser"}/browser-profiles/${taskId}`,
      persistent: false,
      createdAt: new Date().toISOString(),
    }).catch(() => undefined);
  }
}
