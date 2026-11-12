/**
 * Gateway-managed persistent browser profiles (TypeScript counterpart of
 * `crates/browser_use/src/profile.rs`).
 *
 * Layout: `<root>/<sanitized-name>/` holding a Chromium `user_data_dir`
 * (`session.json` marker with `persistent: true`, `passwords: "never"`).
 * The default root is `~/.talos/browser_profiles`, overridable with
 * `TALOS_BROWSER_PROFILE_ROOT`. Profiles are per-*name*, not per-task, so
 * cookies and session storage survive across tasks until cleared.
 */
import { homedir, tmpdir } from "node:os";
import { join } from "node:path";
import {
  mkdir,
  readFile,
  readdir,
  rm,
  stat,
  writeFile,
} from "node:fs/promises";

/** Default profile name reused across tasks. */
export const DEFAULT_PROFILE_NAME = "default";
/** Environment override for the profile root (tests, portable installs). */
export const PROFILE_ROOT_ENV = "TALOS_BROWSER_PROFILE_ROOT";
/** Single profile selected for persistent launch. */
export const PROFILE_NAME_ENV = "TALOS_BROWSER_PROFILE";
/** Marker file written into every managed profile directory. */
export const PROFILE_MARKER_FILE = "session.json";

/** Gateway-managed profile: named, persistent Chromium `user_data_dir`. */
export interface BrowserProfile {
  name: string;
  userDataDir: string;
  persistent: boolean;
  createdAt: string;
}

/** Resolve the Gateway profile root (env override, else `~/.talos`). */
export function gatewayProfileRoot(): string {
  const override = (process.env[PROFILE_ROOT_ENV] ?? "").trim();
  if (override.length > 0) {
    return override;
  }
  const talosData = (process.env["TALOS_DATA_DIR"] ?? "").trim();
  if (talosData.length > 0) {
    return join(talosData, "browser_profiles");
  }
  try {
    return join(homedir(), ".talos", "browser_profiles");
  } catch {
    return join(tmpdir(), "talos", "browser_profiles");
  }
}

/** Validate + sanitize a profile name (`1..=64` chars, `[a-zA-Z0-9_-]`). */
export function sanitizeProfileName(name: unknown): string {
  if (typeof name !== "string" || name.length === 0 || name.length > 64) {
    throw new Error("profile name must be 1..=64 chars");
  }
  if (!/^[A-Za-z0-9_-]+$/.test(name)) {
    throw new Error("profile name allowed chars: a-z A-Z 0-9 - _");
  }
  return name;
}

/** Directory for one named profile under `root`. */
export function profileDir(root: string, name: string): string {
  return join(root, sanitizeProfileName(name));
}

/** Ensure a persistent profile exists (idempotent; keeps cookies/storage). */
export async function ensureProfile(
  name: string,
  root?: string | undefined,
): Promise<BrowserProfile> {
  const clean = sanitizeProfileName(name);
  const dir = join(root ?? gatewayProfileRoot(), clean);
  await mkdir(dir, { recursive: true });
  let createdAt = new Date().toISOString();
  try {
    const raw = await readFile(join(dir, PROFILE_MARKER_FILE), "utf8");
    const parsed = JSON.parse(raw) as { createdAt?: unknown };
    if (typeof parsed.createdAt === "string" && parsed.createdAt.length > 0) {
      createdAt = parsed.createdAt;
    }
  } catch {
    // Fresh profile: the marker below stamps creation time.
  }
  await writeFile(
    join(dir, PROFILE_MARKER_FILE),
    JSON.stringify(
      { name: clean, persistent: true, passwords: "never", createdAt },
      null,
      2,
    ),
    "utf8",
  );
  return { name: clean, userDataDir: dir, persistent: true, createdAt };
}

/** List Gateway-managed profiles under `root` (missing root ⇒ empty). */
export async function listProfiles(
  root?: string | undefined,
): Promise<BrowserProfile[]> {
  const base = root ?? gatewayProfileRoot();
  let entries: string[];
  try {
    entries = await readdir(base);
  } catch {
    return [];
  }
  const out: BrowserProfile[] = [];
  for (const name of entries.sort()) {
    try {
      sanitizeProfileName(name);
    } catch {
      continue;
    }
    const dir = join(base, name);
    try {
      const info = await stat(dir);
      if (!info.isDirectory()) {
        continue;
      }
    } catch {
      continue;
    }
    let createdAt = new Date().toISOString();
    try {
      const raw = await readFile(join(dir, PROFILE_MARKER_FILE), "utf8");
      const parsed = JSON.parse(raw) as { createdAt?: unknown };
      if (typeof parsed.createdAt === "string" && parsed.createdAt.length > 0) {
        createdAt = parsed.createdAt;
      }
    } catch {
      // No marker yet: report now as creation time.
    }
    out.push({ name, userDataDir: dir, persistent: true, createdAt });
  }
  return out;
}

/**
 * Clear a profile: delete its `user_data_dir` (cookies + storage gone).
 * Returns `true` when a directory was removed. Never touches paths outside
 * the profile root (names are sanitized, traversal impossible).
 */
export async function clearProfile(
  name: string,
  root?: string | undefined,
): Promise<boolean> {
  const dir = join(root ?? gatewayProfileRoot(), sanitizeProfileName(name));
  try {
    await rm(dir, { recursive: true, force: false });
    return true;
  } catch (error) {
    if ((error as NodeJS.ErrnoException | undefined)?.code === "ENOENT") {
      return false;
    }
    throw new Error(
      `browser profile clear: ${error instanceof Error ? error.message : "failed"}`,
    );
  }
}
