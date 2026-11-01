import { createHash } from "node:crypto";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import {
  LiveBrowserAdapter,
  type LiveBrowserContext,
  type LiveLocator,
  type RichLivePage,
} from "./live.js";

const sha256 = (text: string): string =>
  `sha256:${createHash("sha256").update(text, "utf8").digest("hex")}`;

interface FakeOptions {
  html?: string | undefined;
  title?: string | undefined;
  counts?: Record<string, number> | undefined;
  failAction?: string | undefined;
  failName?: string | undefined;
}

class FakeLocator implements LiveLocator {
  constructor(
    private readonly page: FakePage,
    private readonly selector: string,
  ) {}

  async count(): Promise<number> {
    if (this.selector.includes("missing")) {
      return 0;
    }
    return this.page.countFor(this.selector);
  }

  first(): LiveLocator {
    return this;
  }

  async isVisible(): Promise<boolean> {
    return (
      (await this.count()) > 0 &&
      !this.selector.includes("hidden") &&
      !this.selector.includes("disabled")
    );
  }

  async isEnabled(): Promise<boolean> {
    return (await this.isVisible()) && !this.selector.includes("disabled");
  }

  async click(): Promise<void> {
    this.page.recorded.push(`click:${this.selector}`);
    this.page.maybeFail("click");
  }

  async fill(text: string): Promise<void> {
    this.page.recorded.push(`fill:${this.selector}:${text}`);
    this.page.maybeFail("fill");
  }

  async press(key: string): Promise<void> {
    this.page.recorded.push(`press:${key}`);
    this.page.maybeFail("press");
  }

  async selectOption(
    value: string | ReadonlyArray<string>,
  ): Promise<readonly string[]> {
    this.page.recorded.push(`select:${this.selector}:${String(value)}`);
    this.page.maybeFail("select");
    return [String(value)];
  }

  async scrollIntoViewIfNeeded(): Promise<void> {
    this.page.recorded.push(`scrollIntoView:${this.selector}`);
  }

  async textContent(): Promise<string | null> {
    return `text of ${this.selector}`;
  }

  async getAttribute(name: string): Promise<string | null> {
    return name === "type" && this.selector.includes("password")
      ? "password"
      : null;
  }

  async screenshot(): Promise<Uint8Array> {
    return new Uint8Array([7, 7, 7]);
  }
}

class FakePage implements RichLivePage {
  currentUrl = "about:blank";
  readonly recorded: string[] = [];

  constructor(private readonly options: FakeOptions = {}) {}

  countFor(selector: string): number {
    const override = this.options.counts?.[selector];
    if (override !== undefined) {
      return override;
    }
    return 1;
  }

  maybeFail(action: string): void {
    if (this.options.failAction === action) {
      const error = new Error(this.options.failName ?? "boom <secret-content>");
      error.name = this.options.failName ?? "Error";
      throw error;
    }
  }

  url(): string {
    return this.currentUrl;
  }

  async title(): Promise<string> {
    return this.options.title ?? "Fake page";
  }

  async goto(url: string): Promise<unknown> {
    if (this.options.failAction === "goto") {
      const error = new Error(this.options.failName ?? "net::FAILED");
      error.name = this.options.failName ?? "Error";
      throw error;
    }
    this.currentUrl = url;
    this.recorded.push(`goto:${url}`);
    return null;
  }

  locator(selector: string): LiveLocator {
    return new FakeLocator(this, selector);
  }

  async evaluate<T>(script: string): Promise<T> {
    this.recorded.push(`evaluate:${script.slice(0, 24)}`);
    if (script.includes("innerText")) {
      return "hello world" as T;
    }
    return undefined as T;
  }

  async content(): Promise<string> {
    return this.options.html ?? "<html><body>hi</body></html>";
  }

  async screenshot(): Promise<Uint8Array> {
    return new Uint8Array([1, 2, 3, 4]);
  }

  mouse = {
    wheel: async (x: number, y: number): Promise<void> => {
      this.recorded.push(`wheel:${x},${y}`);
    },
  };
}

function fakeContext(page: FakePage): LiveBrowserContext {
  return {
    pages: () => [page],
    newPage: async () => page,
    close: async () => undefined,
  };
}

function adapterWith(
  page: FakePage,
  options?: {
    sensitiveDomains?: string[] | undefined;
    allowedOrigins?: string[] | undefined;
  },
): LiveBrowserAdapter {
  return new LiveBrowserAdapter({
    launch: async () => fakeContext(page),
    profileRoot: mkdtempSync(join(tmpdir(), "talos-live-")),
    sensitiveDomains: options?.sensitiveDomains,
    allowedOrigins: options?.allowedOrigins,
  });
}

describe("live browser adapter", () => {
  it("navigates and hashes the real DOM", async () => {
    const page = new FakePage({ title: "Example" });
    const adapter = adapterWith(page);
    const result = await adapter.executeAction({
      action: "navigate",
      url: "https://example.com/",
      taskId: "task-1",
    });
    expect(result.status).toBe("success");
    expect(result.title).toBe("Example");
    expect(result.targetUrl).toBe("https://example.com/");
    expect(result.domHash).toBe(sha256("<html><body>hi</body></html>"));
    expect(result.simulated).toBe(false);
    expect(adapter.sessionCount()).toBe(1);
    await adapter.closeAllSessions();
  });

  it("blocks privileged schemes before launching sessions", async () => {
    const page = new FakePage();
    const adapter = adapterWith(page);
    const result = await adapter.executeAction({
      action: "navigate",
      url: "file:///etc/passwd",
      taskId: "task-2",
    });
    expect(result.status).toBe("failed");
    expect(result.error?.code).toBe("blocked_scheme");
    expect(adapter.sessionCount()).toBe(0);
    expect(page.recorded).toEqual([]);
  });

  it("enforces the origin allowlist", async () => {
    const page = new FakePage();
    const adapter = adapterWith(page, {
      allowedOrigins: ["https://example.com"],
    });
    const result = await adapter.executeAction({
      action: "navigate",
      url: "https://evil.test/",
      taskId: "task-3",
    });
    expect(result.status).toBe("failed");
    expect(result.error?.code).toBe("origin_not_allowed");
  });

  it("resolves selectors before mutating", async () => {
    const page = new FakePage();
    const adapter = adapterWith(page);
    const missing = await adapter.executeAction({
      action: "click",
      url: "https://example.com/",
      selector: ".missing-thing",
      taskId: "task-4",
    });
    expect(missing.status).toBe("failed");
    expect(missing.error?.code).toBe("not_found");
    expect(missing.resolved?.matchCount).toBe(0);
    expect(
      page.recorded.filter((entry) => entry.startsWith("click:")),
    ).toHaveLength(0);

    const hidden = await adapter.executeAction({
      action: "click",
      url: "https://example.com/",
      selector: ".hidden-cta",
      taskId: "task-4",
    });
    expect(hidden.error?.code).toBe("not_visible");
  });

  it("requires approval for high-risk mutations", async () => {
    const page = new FakePage();
    const adapter = adapterWith(page);
    const denied = await adapter.executeAction({
      action: "click",
      url: "https://example.com/",
      selector: "button#ok",
      highRisk: true,
      highRiskKind: "form_submit",
      taskId: "task-5",
    });
    expect(denied.status).toBe("denied");
    expect(denied.error?.code).toBe("approval_required");
    const allowed = await adapter.executeAction({
      action: "click",
      url: "https://example.com/",
      selector: "button#ok",
      highRisk: true,
      highRiskKind: "form_submit",
      approved: true,
      taskId: "task-5",
    });
    expect(allowed.status).toBe("success");
    expect(allowed.resolved?.interactable).toBe(true);
  });

  it("never auto-fills password fields", async () => {
    const page = new FakePage();
    const adapter = adapterWith(page);
    const result = await adapter.executeAction({
      action: "type",
      url: "https://example.com/login",
      selector: 'input[type="password"]',
      text: "hunter2",
      taskId: "task-6",
    });
    expect(result.status).toBe("denied");
    expect(result.error?.code).toBe("auth_required");
    expect(
      page.recorded.filter((entry) => entry.startsWith("fill:")),
    ).toHaveLength(0);
  });

  it("marks sensitive pages and skips artifact storage", async () => {
    const page = new FakePage();
    const adapter = adapterWith(page, { sensitiveDomains: ["bank.test"] });
    const navigated = await adapter.executeAction({
      action: "navigate",
      url: "https://bank.test/",
      taskId: "task-7",
    });
    expect(navigated.sensitive).toBe(true);
    const shot = await adapter.executeAction({
      action: "screenshot",
      url: "https://bank.test/",
      taskId: "task-7",
      thumbnail: true,
    });
    expect(shot.status).toBe("success");
    expect(shot.screenshotHash).toMatch(/^sha256:/);
    expect(shot.screenshotPath).toBeUndefined();
    expect(shot.thumbnailDataUrl).toBeUndefined();
  });

  it("redacts password regions before hashing", async () => {
    const page = new FakePage({
      html: '<html><body><input type="password"></body></html>',
    });
    const adapter = adapterWith(page);
    const shot = await adapter.executeAction({
      action: "screenshot",
      url: "https://example.com/",
      taskId: "task-8",
    });
    expect(shot.status).toBe("success");
    expect(shot.redacted).toBe(true);
    expect(shot.screenshotHash).toMatch(/^sha256:/);
  });

  it("returns bounded thumbnails on request", async () => {
    const page = new FakePage();
    const adapter = adapterWith(page);
    const shot = await adapter.executeAction({
      action: "screenshot",
      url: "https://example.com/",
      taskId: "task-9",
      thumbnail: true,
    });
    expect(shot.thumbnailDataUrl?.startsWith("data:image/jpeg;base64,")).toBe(
      true,
    );
  });

  it("caps extracted text and hashes the full body", async () => {
    const page = new FakePage();
    const adapter = adapterWith(page);
    const result = await adapter.executeAction({
      action: "extract_text",
      url: "https://example.com/",
      taskId: "task-10",
      maxChars: 5,
    });
    expect(result.extractedText).toBe("hello");
    expect(result.truncated).toBe(true);
    expect(result.extractedTextHash).toBe(sha256("hello world"));
  });

  it("maps timeouts without leaking page content", async () => {
    const page = new FakePage({ failAction: "goto", failName: "TimeoutError" });
    const adapter = adapterWith(page);
    const result = await adapter.executeAction({
      action: "navigate",
      url: "https://slow.test/",
      taskId: "task-11",
    });
    expect(result.status).toBe("failed");
    expect(result.error?.code).toBe("timeout");
  });

  it("sanitizes action failure messages", async () => {
    const page = new FakePage({ failAction: "click" });
    const adapter = adapterWith(page);
    const result = await adapter.executeAction({
      action: "click",
      url: "https://example.com/",
      selector: "button#ok",
      taskId: "task-12",
    });
    expect(result.status).toBe("failed");
    expect(result.error?.message ?? "").not.toContain("secret-content");
  });
});
