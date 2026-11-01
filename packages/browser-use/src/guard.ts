/**
 * Navigation and credential guards for live browser execution.
 *
 * Defense in depth: the Rust tool layer blocks forbidden schemes before the
 * worker ever spawns; these checks run again inside the worker so a raw
 * JSON-RPC caller gets the same refusals.
 */

export const FORBIDDEN_SCHEMES = [
  "file:",
  "chrome:",
  "chrome-extension:",
  "about:",
  "javascript:",
  "data:",
  "view-source:",
] as const;

/** Machine-readable guard failure codes (also used in Rust receipts). */
export type GuardErrorCode =
  "blocked_scheme" | "origin_not_allowed" | "auth_required" | "sensitive_page";

/** Structured guard failure: code only, never page content. */
export class BrowserGuardError extends Error {
  readonly code: GuardErrorCode;

  constructor(code: GuardErrorCode, message: string) {
    super(message);
    this.name = "BrowserGuardError";
    this.code = code;
  }
}

/**
 * Assert a URL is navigable: http(s) only, never a privileged scheme.
 * Returns the lower-cased origin (`https://host[:port]`) for allowlist checks.
 */
export function assertNavigableUrl(
  raw: string,
  allowedOrigins?: readonly string[] | undefined,
): string {
  let parsed: URL;
  try {
    parsed = new URL(raw);
  } catch {
    throw new BrowserGuardError(
      "blocked_scheme",
      "navigation target is not a URL",
    );
  }
  const scheme = `${parsed.protocol}`;
  if ((FORBIDDEN_SCHEMES as readonly string[]).includes(scheme)) {
    throw new BrowserGuardError(
      "blocked_scheme",
      `navigation to ${scheme} URLs is blocked`,
    );
  }
  if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
    throw new BrowserGuardError(
      "blocked_scheme",
      "navigation is restricted to http(s)",
    );
  }
  const origin = parsed.origin.toLowerCase();
  if (allowedOrigins !== undefined && allowedOrigins.length > 0) {
    const allowed = allowedOrigins.map((entry) => entry.toLowerCase());
    if (!allowed.includes(origin)) {
      throw new BrowserGuardError(
        "origin_not_allowed",
        `origin ${origin} is outside the task allowlist`,
      );
    }
  }
  return origin;
}

/** Operator-configured sensitive domains (comma list, host or parent match). */
export function sensitiveHosts(): string[] {
  const raw = process.env["TALOS_BROWSER_SENSITIVE_DOMAINS"] ?? "";
  return raw
    .split(",")
    .map((entry) => entry.trim().toLowerCase())
    .filter((entry) => entry.length > 0);
}

/** Task allowlist from the environment (empty = any http(s) origin). */
export function allowedOrigins(): string[] {
  const raw = process.env["TALOS_BROWSER_ALLOWED_ORIGINS"] ?? "";
  return raw
    .split(",")
    .map((entry) => entry.trim().toLowerCase())
    .filter((entry) => entry.length > 0);
}

/** True when the URL's host matches the sensitive blocklist. */
export function isSensitiveUrl(
  raw: string,
  blocklist: readonly string[] = sensitiveHosts(),
): boolean {
  let host: string;
  try {
    host = new URL(raw).hostname.toLowerCase();
  } catch {
    return true;
  }
  return blocklist.some(
    (entry) => host === entry || host.endsWith(`.${entry}`),
  );
}

/**
 * True when typing into this target would inject credentials.
 * Password fields are never auto-filled: the worker refuses and the caller
 * must raise `BrowserAuthRequired` for human handling instead.
 */
export function isCredentialTarget(selector?: string | undefined): boolean {
  if (selector === undefined) {
    return false;
  }
  const lowered = selector.toLowerCase();
  return (
    lowered.includes("password") ||
    lowered.includes("passwd") ||
    lowered.includes('type="password"') ||
    lowered.includes("type='password'")
  );
}
