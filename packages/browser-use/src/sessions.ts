import type { LiveBrowserContext } from "./live.js";
import { closeSession, createSession, type BrowserSession } from "./session.js";

/** One warm browser session bound to a task. */
export interface ManagedSession {
  /** Task linkage. */
  taskId: string;
  /** Isolated session (profile dir). */
  session: BrowserSession;
  /** Live browser context. */
  context: LiveBrowserContext;
  /** Last-known URL (best effort). */
  currentUrl?: string | undefined;
  /** Last-known title (best effort). */
  title?: string | undefined;
  /** Last activity epoch ms. */
  lastActivityMs: number;
}

/** Launcher signature: real Playwright by default, fakes in tests. */
export type ContextLauncher = (
  profileDir: string,
) => Promise<LiveBrowserContext>;

/** Session manager options. */
export interface SessionManagerOptions {
  /** Max concurrent warm sessions (default 1 for safety). */
  maxSessions?: number | undefined;
  /** Idle timeout in ms before auto-close (default 5 minutes). */
  idleTimeoutMs?: number | undefined;
  /** Context launcher (inject fakes in tests). */
  launch?: ContextLauncher | undefined;
  /** Clock (inject fakes in tests). */
  now?: (() => number) | undefined;
}

/**
 * Warm-session manager: at most `maxSessions` live contexts, LRU-evicted
 * idle sessions auto-close after `idleTimeoutMs`. `closeAll` is the
 * emergency-stop path; every close is idempotent.
 */
export class LiveSessionManager {
  private readonly sessions = new Map<string, ManagedSession>();
  private readonly maxSessions: number;
  private readonly idleTimeoutMs: number;
  private readonly launch: ContextLauncher;
  private readonly now: () => number;

  constructor(options?: SessionManagerOptions | undefined) {
    const max = options?.maxSessions ?? 1;
    if (!Number.isInteger(max) || max < 1) {
      throw new Error("maxSessions must be a positive integer");
    }
    const idle = options?.idleTimeoutMs ?? 5 * 60 * 1000;
    if (!Number.isFinite(idle) || idle <= 0) {
      throw new Error("idleTimeoutMs must be positive");
    }
    if (options?.launch === undefined) {
      throw new Error("a context launcher is required");
    }
    this.maxSessions = max;
    this.idleTimeoutMs = idle;
    this.launch = options.launch;
    this.now = options.now ?? Date.now;
  }

  /** Active warm session count. */
  count(): number {
    return this.sessions.size;
  }

  /** Get a warm session (touches activity) or undefined. */
  get(taskId: string): ManagedSession | undefined {
    const found = this.sessions.get(taskId);
    if (found !== undefined) {
      found.lastActivityMs = this.now();
    }
    return found;
  }

  /**
   * Launch (or reuse) a session for a task. Throws `session_limit` when a
   * different task is warm and the cap is reached; callers close first.
   */
  async launchSession(
    taskId: string,
    profileDir?: string | undefined,
  ): Promise<ManagedSession> {
    if (!taskId || taskId.length > 128) {
      throw new Error("taskId must be 1..=128 chars");
    }
    const existing = this.sessions.get(taskId);
    if (existing !== undefined) {
      existing.lastActivityMs = this.now();
      return existing;
    }
    if (this.sessions.size >= this.maxSessions) {
      const error = new Error(
        `session limit reached (${this.maxSessions} warm session(s))`,
      );
      (error as NodeJS.ErrnoException).code = "session_limit";
      throw error;
    }
    const session = await createSession(taskId, { root: profileDir });
    const context = await this.launch(session.profileDir);
    const managed: ManagedSession = {
      taskId,
      session,
      context,
      lastActivityMs: this.now(),
    };
    this.sessions.set(taskId, managed);
    return managed;
  }

  /** Refresh activity and snapshot the current URL/title. */
  touch(taskId: string, currentUrl?: string, title?: string): void {
    const found = this.sessions.get(taskId);
    if (found === undefined) {
      return;
    }
    found.lastActivityMs = this.now();
    if (currentUrl !== undefined) {
      found.currentUrl = currentUrl;
    }
    if (title !== undefined) {
      found.title = title;
    }
  }

  /** Close one session (context first, profile per persistence flag). */
  async closeSession(taskId: string): Promise<boolean> {
    const found = this.sessions.get(taskId);
    if (found === undefined) {
      return false;
    }
    this.sessions.delete(taskId);
    await found.context.close().catch(() => undefined);
    await closeSession(found.session).catch(() => undefined);
    return true;
  }

  /** Close every warm session; returns the closed task ids. */
  async closeAll(): Promise<string[]> {
    const ids = [...this.sessions.keys()];
    for (const id of ids) {
      await this.closeSession(id);
    }
    return ids;
  }

  /**
   * Close sessions idle longer than the timeout. Returns closed task ids.
   * Runs on a sweeper or on demand before launching.
   */
  async sweepIdle(): Promise<string[]> {
    const cutoff = this.now() - this.idleTimeoutMs;
    const stale = [...this.sessions.values()]
      .filter((session) => session.lastActivityMs < cutoff)
      .map((session) => session.taskId);
    for (const id of stale) {
      await this.closeSession(id);
    }
    return stale;
  }
}
