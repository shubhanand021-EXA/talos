import { describe, expect, it } from "vitest";
import { nextLiteMode } from "./lite-mode.js";

describe("auto lite-mode governor", () => {
  it("triggers on sustained overload and recovers on health", () => {
    let samples = [60, 60];
    let state = false;
    for (const fps of [24, 22, 18, 15]) {
      samples = [...samples, fps];
      state = nextLiteMode({
        fpsSamples: samples,
        current: state,
        override: "auto",
      }).lite;
    }
    expect(state).toBe(true);

    // One healthy window is not enough (hysteresis holds lite).
    const hold = nextLiteMode({
      fpsSamples: [60, 60, 60, 60, 60],
      current: true,
      override: "auto",
    });
    expect(hold).toEqual({ lite: false, reason: "auto-recover" });

    // A single dip never triggers (needs 3 consecutive sub-30 windows).
    expect(
      nextLiteMode({
        fpsSamples: [60, 60, 12],
        current: false,
        override: "auto",
      }).lite,
    ).toBe(false);
    expect(
      nextLiteMode({
        fpsSamples: [60, 12, 60],
        current: false,
        override: "auto",
      }).lite,
    ).toBe(false);
    expect(
      nextLiteMode({
        fpsSamples: [12, 11, 10],
        current: false,
        override: "auto",
      }),
    ).toEqual({ lite: true, reason: "auto-trigger" });
  });

  it("manual override wins over the governor either way", () => {
    expect(
      nextLiteMode({
        fpsSamples: [60, 60, 60],
        current: false,
        override: "lite",
      }),
    ).toEqual({ lite: true, reason: "manual-lite" });
    expect(
      nextLiteMode({ fpsSamples: [5, 5, 5], current: true, override: "full" }),
    ).toEqual({ lite: false, reason: "manual-full" });
  });

  it("is pure: frozen input, repeated calls, identical decisions", () => {
    const input = Object.freeze({
      fpsSamples: Object.freeze([20, 21, 19]),
      current: false,
      override: "auto",
    }) as unknown as Parameters<typeof nextLiteMode>[0];
    const first = nextLiteMode(input);
    for (let i = 0; i < 10_000; i += 1) {
      expect(nextLiteMode(input)).toEqual(first);
    }
    expect(first).toEqual({ lite: true, reason: "auto-trigger" });
  });
});
