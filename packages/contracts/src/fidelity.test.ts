import { describe, expect, it } from "vitest";
import {
  ExecutionFidelitySchema,
  FidelityMismatchError,
  assertFidelity,
  minFidelity,
} from "./fidelity.js";

describe("execution fidelity", () => {
  it("parses the three fidelity states and rejects the rest", () => {
    expect(ExecutionFidelitySchema.parse("mock")).toBe("mock");
    expect(ExecutionFidelitySchema.parse("live")).toBe("live");
    expect(ExecutionFidelitySchema.parse("verified")).toBe("verified");
    expect(() => ExecutionFidelitySchema.parse("real")).toThrow();
    expect(() => ExecutionFidelitySchema.parse("")).toThrow();
  });

  it("orders mock below live below verified", () => {
    expect(minFidelity("mock", "verified")).toBe("mock");
    expect(minFidelity("live", "verified")).toBe("live");
    expect(minFidelity("verified", "verified")).toBe("verified");
    expect(minFidelity("live", "mock")).toBe("mock");
  });

  it("allows honest claims and rejects mock presented as verified", () => {
    expect(() => assertFidelity("mock", "mock")).not.toThrow();
    expect(() => assertFidelity("verified", "live")).not.toThrow();
    expect(() => assertFidelity("mock", "verified")).toThrow(
      FidelityMismatchError,
    );
    expect(() => assertFidelity("live", "verified")).toThrow(
      FidelityMismatchError,
    );
    try {
      assertFidelity("mock", "live");
      expect.unreachable("mock as live must throw");
    } catch (error) {
      expect(error).toBeInstanceOf(FidelityMismatchError);
      const mismatch = error as FidelityMismatchError;
      expect(mismatch.actual).toBe("mock");
      expect(mismatch.claimed).toBe("live");
      expect(mismatch.message).toContain("mock");
    }
  });
});
