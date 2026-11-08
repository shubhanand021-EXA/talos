/**
 * Human-in-the-loop CAPTCHA/auth bridge (TypeScript counterpart of
 * `crates/browser_use/src/hitl.rs`).
 *
 * When the live adapter detects a CAPTCHA or login wall (heuristics below,
 * or an explicit `browser.wait_for_human` call), execution PAUSES on a gate
 * until the Desktop UI resolves it (`browser.resume_human` with
 * `resumed: true|false`). The UI shows the intervention modal over a
 * foreground Playwright window on the same `user_data_dir`.
 *
 * Events and receipts carry task linkage, URLs, and reason tokens only —
 * never page content, credentials, or challenge images.
 */

/** Why the browser agent paused for a human. */
export type HitlKind = "captcha_detected" | "auth_required";

/** UI-bound intervention event (mirrors `HitlBrowserEventSchema`). */
export interface HitlIntervention {
  interventionId: string;
  taskId: string;
  kind: HitlKind;
  url?: string | undefined;
  reason: string;
  at: string;
}

/** Signals the live page check observes. */
export interface InterventionSignals {
  url?: string | undefined;
  title?: string | undefined;
  html?: string | undefined;
}

/**
 * Heuristic CAPTCHA / login-wall detection over page signals.
 * Returns the kind plus a short content-free reason token, or `null` when
 * the page looks clean. CAPTCHA markers win over login forms (a login form
 * behind Cloudflare is a CAPTCHA first). The scan is bounded to the first
 * 32 KiB so adversarial pages cannot stall the agent loop.
 */
export function detectIntervention(
  signals: InterventionSignals,
): { kind: HitlKind; reason: string } | null {
  const haystack =
    `${signals.url ?? ""} ${signals.title ?? ""} ${signals.html ?? ""}`.toLowerCase();
  const scan = haystack.slice(0, 32_768);
  const captcha: Array<[string, string]> = [
    ["cf-challenge", "cloudflare-challenge"],
    ["cloudflare", "cloudflare-challenge"],
    ["__cf_chl", "cloudflare-challenge"],
    ["turnstile", "turnstile-challenge"],
    ["recaptcha", "recaptcha-challenge"],
    ["g-recaptcha", "recaptcha-challenge"],
    ["hcaptcha", "hcaptcha-challenge"],
    ["h-captcha", "hcaptcha-challenge"],
    ["captcha", "captcha-challenge"],
    ["verify you are human", "human-verification"],
    ["verify you're human", "human-verification"],
    ["are you a robot", "human-verification"],
    ["datadome", "datadome-challenge"],
    ["perimeterx", "perimeterx-challenge"],
    ["kasada", "kasada-challenge"],
  ];
  for (const [marker, reason] of captcha) {
    if (scan.includes(marker)) {
      return { kind: "captcha_detected", reason };
    }
  }
  const auth: Array<[string, string]> = [
    ["input[type=password", "password-field"],
    ['type="password"', "password-field"],
    ["sso", "sso-redirect"],
    ["single sign-on", "sso-redirect"],
    ["sign in to continue", "login-wall"],
    ["log in to continue", "login-wall"],
    ["session expired", "session-expired"],
    ["please log in", "login-wall"],
  ];
  for (const [marker, reason] of auth) {
    if (scan.includes(marker)) {
      return { kind: "auth_required", reason };
    }
  }
  return null;
}

/** Build the UI-bound event for a pause request (reason capped at 256). */
export function hitlEvent(
  taskId: string,
  kind: HitlKind,
  reason: string,
  url?: string | undefined,
): HitlIntervention {
  return {
    interventionId: `hitl-${Date.now().toString(36)}-${Math.floor(Math.random() * 0xffffff).toString(36)}`,
    taskId,
    kind,
    url,
    reason: reason.slice(0, 256),
    at: new Date().toISOString(),
  };
}
