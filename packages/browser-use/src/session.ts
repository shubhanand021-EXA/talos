import { mkdir, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

/** Isolated browser session: dedicated profile dir, never a personal profile. */
export interface BrowserSession {
  /** Session identifier. */
  id: string;
  /** Owning task (ledger linkage). */
  taskId: string;
  /** Dedicated profile directory under the sandbox root. */
  profileDir: string;
  /** Cookies/history persist only when explicitly enabled. */
  persistent: boolean;
  /** RFC 3339 creation timestamp. */
  createdAt: string;
}

function sanitize(taskId: string): string {
  return taskId
    .split("")
    .map((c) => (/[A-Za-z0-9_-]/.test(c) ? c : "_"))
    .join("");
}

/** Default sandbox root for browser profiles (overridable per call). */
export function defaultRoot(): string {
  return join(tmpdir(), "talos-browser");
}

/**
 * Create an isolated session: fresh profile dir + session marker.
 * Never touches personal profiles; no passwords are ever stored.
 */
export async function createSession(
  taskId: string,
  options?: { root?: string | undefined; persistent?: boolean | undefined },
): Promise<BrowserSession> {
  if (!taskId || taskId.length > 128) {
    throw new Error("taskId must be 1..=128 chars");
  }
  const persistent = options?.persistent ?? false;
  const profileDir = join(
    options?.root ?? defaultRoot(),
    "browser-profiles",
    sanitize(taskId),
  );
  await mkdir(profileDir, { recursive: true });
  const session: BrowserSession = {
    id: `session-${sanitize(taskId)}`,
    taskId,
    profileDir,
    persistent,
    createdAt: new Date().toISOString(),
  };
  await writeFile(
    join(profileDir, "session.json"),
    JSON.stringify({ ...session, passwords: "never" }, null, 2),
    "utf8",
  );
  return session;
}

/** Close a session: remove the profile dir unless it was marked persistent. */
export async function closeSession(session: BrowserSession): Promise<void> {
  if (session.persistent) {
    return;
  }
  await rm(session.profileDir, { recursive: true, force: true });
}

/** True when the session profile exists on disk. */
export async function sessionExists(session: BrowserSession): Promise<boolean> {
  try {
    const info = await stat(session.profileDir);
    return info.isDirectory();
  } catch {
    return false;
  }
}
