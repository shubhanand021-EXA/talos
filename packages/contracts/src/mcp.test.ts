import { describe, expect, it } from "vitest";
import {
  InstructionParseViewSchema,
  McpServerConfigSchema,
  McpServerInfoSchema,
  McpToolCallReceiptSchema,
  McpToolSchema,
  PermissionReviewSchema,
  PluginInfoSchema,
  PluginManifestSchema,
} from "./mcp.js";

describe("mcp contracts", () => {
  it("accepts a stdio server config", () => {
    const parsed = McpServerConfigSchema.safeParse({
      command: "npx",
      args: ["-y", "mcp-server"],
      env: {},
      transport: "stdio",
      capabilities: ["tools"],
    });
    expect(parsed.success).toBe(true);
  });

  it("accepts an SSE server config", () => {
    const parsed = McpServerConfigSchema.safeParse({
      transport: "sse",
      url: "https://example.com/mcp",
      capabilities: ["tools", "resources"],
    });
    expect(parsed.success).toBe(true);
  });

  it("rejects configs without capabilities", () => {
    const parsed = McpServerConfigSchema.safeParse({
      command: "npx",
      capabilities: [],
    });
    expect(parsed.success).toBe(false);
  });

  it("round-trips server info, tools, and receipts", () => {
    expect(
      McpServerInfoSchema.safeParse({
        name: "fs",
        transport: "stdio",
        capabilities: ["tools"],
        timeoutMs: 30000,
        status: "disconnected",
        trust: "trusted",
      }).success,
    ).toBe(true);
    expect(
      McpToolSchema.safeParse({
        name: "read",
        description: "read a file",
        inputSchema: { type: "object" },
      }).success,
    ).toBe(true);
    expect(
      McpToolCallReceiptSchema.safeParse({
        id: "mcp-receipt-1",
        server: "fs",
        tool: "read",
        taskId: "task-1",
        decision: "Allow",
        reason: "allowed",
        output: null,
        isError: false,
      }).success,
    ).toBe(true);
  });

  it("round-trips instruction views, manifests, reviews, and plugins", () => {
    expect(
      InstructionParseViewSchema.safeParse({
        path: "AGENTS.md",
        kind: "agents",
        summary: {},
      }).success,
    ).toBe(true);
    expect(
      PluginManifestSchema.safeParse({
        name: "fs-pack",
        version: "0.1.0",
        author: "talos",
        capabilities: ["tools"],
        permissions: ["fs.read"],
      }).success,
    ).toBe(true);
    expect(
      PermissionReviewSchema.safeParse({
        plugin: "fs-pack 0.1.0",
        requested: ["fs.read"],
        capabilities: ["tools"],
        unsigned: true,
        signatureValid: false,
      }).success,
    ).toBe(true);
    expect(
      PluginInfoSchema.safeParse({
        name: "fs-pack",
        version: "0.1.0",
        kind: "plugin",
        trust: "pending",
        capabilities: ["tools"],
        permissions: ["fs.read"],
        unsigned: true,
      }).success,
    ).toBe(true);
  });
});
