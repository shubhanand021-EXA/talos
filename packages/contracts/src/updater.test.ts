import { describe, expect, it } from "vitest";
import { UpdateInfoSchema } from "./updater.js";

describe("updater domain", () => {
  it("parses update info payloads", () => {
    const available = {
      available: true,
      currentVersion: "0.3.0",
      latestVersion: "0.4.0",
      notes: "Hardening release",
      pubDate: "2026-11-02T00:00:00.000Z",
      downloadUrl: "https://updates.talos.dev/x86_64/0.4.0",
    } as const;
    expect(UpdateInfoSchema.parse(available)).toEqual(available);
    const idle = {
      available: false,
      currentVersion: "0.4.0",
    } as const;
    expect(UpdateInfoSchema.parse(idle)).toMatchObject({
      available: false,
    });
    expect(() =>
      UpdateInfoSchema.parse({ ...idle, available: "yes" }),
    ).toThrow();
  });
});
