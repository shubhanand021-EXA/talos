import { describe, expect, it } from "vitest";
import {
  encodeFrame,
  parseFrame,
  RpcEventSchema,
  RpcRequestSchema,
  RpcResponseSchema,
  splitFrames,
} from "./protocol.js";

describe("worker protocol framing", () => {
  it("encodes single-line frames", () => {
    const frame = encodeFrame({ id: "1", ok: true });
    expect(frame.endsWith("\n")).toBe(true);
    expect(frame.trim().split("\n")).toHaveLength(1);
  });

  it("splits streams keeping the remainder", () => {
    const { frames, rest } = splitFrames('{"a":1}\n{"b":2}\n{"c":');
    expect(frames).toHaveLength(2);
    expect(rest).toBe('{"c":');
  });

  it("skips blank lines", () => {
    const { frames, rest } = splitFrames('\n{"a":1}\n\n');
    expect(frames).toHaveLength(1);
    expect(rest).toBe("");
  });

  it("parses frames and rejects garbage", () => {
    expect(parseFrame('{"id":"1"}')).toEqual({ id: "1" });
    expect(() => parseFrame("{nope")).toThrow();
  });

  it("validates request/response/event shapes", () => {
    expect(
      RpcRequestSchema.safeParse({ id: "1", method: "ping" }).success,
    ).toBe(true);
    expect(RpcResponseSchema.safeParse({ id: "1", ok: true }).success).toBe(
      true,
    );
    expect(
      RpcEventSchema.safeParse({ event: "browserAuthRequired" }).success,
    ).toBe(true);
    expect(RpcRequestSchema.safeParse({ method: "ping" }).success).toBe(false);
  });
});
