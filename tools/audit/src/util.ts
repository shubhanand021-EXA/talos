import { execFileSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import type { ExecFn } from "./types.ts";

const SKIP_DIRS = new Set([
  "node_modules",
  ".git",
  "target",
  "dist",
  "gen",
  "coverage",
  ".turbo",
  "ios",
  "android",
]);

const TEXT_EXTENSIONS = new Set([
  ".ts",
  ".tsx",
  ".js",
  ".mjs",
  ".cjs",
  ".json",
  ".toml",
  ".md",
  ".yml",
  ".yaml",
  ".sql",
  ".css",
  ".html",
  ".rs",
  ".txt",
]);

/** True when a path exists (file or directory). */
export function fileExists(path: string): boolean {
  return existsSync(path);
}

/** Read a UTF-8 text file. */
export function readText(path: string): string {
  return readFileSync(path, "utf8");
}

/** Parse a JSON file. */
export function readJson<T>(path: string): T {
  return JSON.parse(readText(path)) as T;
}

/** Yield text files under `dir`, skipping build/vendor directories. */
export function* walkTextFiles(dir: string): Generator<string> {
  let entries: string[];
  try {
    entries = readdirSync(dir);
  } catch {
    return;
  }
  for (const entry of entries) {
    if (SKIP_DIRS.has(entry)) {
      continue;
    }
    const full = join(dir, entry);
    let stat: ReturnType<typeof statSync>;
    try {
      stat = statSync(full);
    } catch {
      continue;
    }
    if (stat.isDirectory()) {
      yield* walkTextFiles(full);
    } else if ([...TEXT_EXTENSIONS].some((ext) => full.endsWith(ext))) {
      yield full;
    }
  }
}

export interface GrepHit {
  file: string;
  line: number;
  text: string;
}

/** Grep one pattern across text files (relative paths in hits).
 *
 * `excludeParts` skips files whose repo-relative path contains any entry
 * (used for pattern-definition files and test fixtures); `*.test.*` files
 * are always skipped so example secrets in tests are not flagged.
 */
export function grepFiles(
  root: string,
  pattern: RegExp,
  dirs?: string[],
  excludeParts?: string[],
): GrepHit[] {
  const hits: GrepHit[] = [];
  const bases = dirs === undefined ? [root] : dirs.map((d) => join(root, d));
  const flags = pattern.flags.includes("g")
    ? pattern.flags
    : `${pattern.flags}g`;
  for (const base of bases) {
    for (const file of walkTextFiles(base)) {
      // Normalize separators so exclusion checks work on every platform.
      const rel = file.slice(root.length + 1).replace(/\\/g, "/");
      if (
        rel.endsWith(".test.ts") ||
        rel.endsWith(".test.tsx") ||
        rel.endsWith(".test.js") ||
        rel.endsWith(".test.mjs") ||
        (excludeParts ?? []).some(
          (part) =>
            rel === part ||
            rel.startsWith(`${part}/`) ||
            rel.includes(`/${part}/`),
        )
      ) {
        continue;
      }
      let content: string;
      try {
        content = readText(file);
      } catch {
        continue;
      }
      const re = new RegExp(pattern.source, flags);
      const lines = content.split("\n");
      for (const [index, line] of lines.entries()) {
        re.lastIndex = 0;
        if (re.test(line)) {
          hits.push({
            file: rel,
            line: index + 1,
            text: line.trim().slice(0, 200),
          });
          if (hits.length >= 50) {
            return hits;
          }
        }
      }
    }
  }
  return hits;
}

/** Default command executor (synchronous, bounded by timeout).
 *
 * On Windows, commands resolve through `cmd.exe` so `.cmd`/`.bat` shims
 * (pnpm, cargo via rustup) work the same as on POSIX shells.
 */
export const defaultExec: ExecFn = (command, args, options) => {
  const invocation: [string, string[]] =
    process.platform === "win32"
      ? ["cmd.exe", ["/d", "/s", "/c", [command, ...args].join(" ")]]
      : [command, args];
  try {
    const stdout = execFileSync(invocation[0], invocation[1], {
      cwd: options?.cwd,
      timeout: options?.timeoutMs ?? 60000,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
      windowsHide: true,
    });
    return { ok: true, stdout, stderr: "", exitCode: 0, timedOut: false };
  } catch (error) {
    const err = error as {
      stdout?: unknown;
      stderr?: unknown;
      status?: unknown;
      code?: unknown;
    };
    const timedOut =
      (typeof err.code === "string" && err.code === "ETIMEDOUT") ||
      (typeof err.code === "number" && err.code === 0 && false);
    return {
      ok: false,
      stdout: typeof err.stdout === "string" ? err.stdout : "",
      stderr: typeof err.stderr === "string" ? err.stderr : String(error),
      exitCode: typeof err.status === "number" ? err.status : null,
      timedOut,
    };
  }
};
