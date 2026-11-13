/**
 * Auto lite-mode: deterministic frame-budget governor for the Void Neon
 * viewport (Prompt 012, Phase B). Pure function — no React, no allocation
 * beyond the returned record — so the trigger/recover behavior is unit
 * tested without a display.
 *
 * Lite-mode reduces effects, never the design language: particle density,
 * post-processing bloom, DPR, event pulses, and orbit motion step down;
 * palette, typography, and layout are untouched.
 */

/** Consecutive sub-budget windows that force lite-mode on. */
export const LITE_TRIGGER_WINDOWS = 3;
/** Sustained-fps threshold that forces lite-mode on (< 30fps). */
export const LITE_TRIGGER_FPS = 30;
/** Consecutive healthy windows that restore full mode. */
export const LITE_RECOVER_WINDOWS = 5;
/** Sustained-fps threshold that restores full mode (>= 50fps). */
export const LITE_RECOVER_FPS = 50;

/** Manual override: `auto` follows the governor, the rest win outright. */
export type LiteOverride = "auto" | "lite" | "full";

export interface LiteModeInput {
  /** Most recent throttled fps samples, oldest first. */
  fpsSamples: number[];
  /** Current lite state (hysteresis anchor). */
  current: boolean;
  /** Manual override from the HUD. */
  override: LiteOverride;
}

export interface LiteModeDecision {
  lite: boolean;
  reason:
    | "manual-lite"
    | "manual-full"
    | "auto-trigger"
    | "auto-recover"
    | "auto-hold";
}

/** Deterministic governor step: same input always yields the same decision. */
export function nextLiteMode(input: LiteModeInput): LiteModeDecision {
  if (input.override === "lite") {
    return { lite: true, reason: "manual-lite" };
  }
  if (input.override === "full") {
    return { lite: false, reason: "manual-full" };
  }
  const { fpsSamples, current } = input;
  if (!current) {
    const tail = fpsSamples.slice(-LITE_TRIGGER_WINDOWS);
    if (
      tail.length >= LITE_TRIGGER_WINDOWS &&
      tail.every((fps) => fps < LITE_TRIGGER_FPS)
    ) {
      return { lite: true, reason: "auto-trigger" };
    }
    return { lite: false, reason: "auto-hold" };
  }
  const tail = fpsSamples.slice(-LITE_RECOVER_WINDOWS);
  if (
    tail.length >= LITE_RECOVER_WINDOWS &&
    tail.every((fps) => fps >= LITE_RECOVER_FPS)
  ) {
    return { lite: false, reason: "auto-recover" };
  }
  return { lite: true, reason: "auto-hold" };
}
