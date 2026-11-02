import { z } from "zod";
import { IsoDateTimeSchema } from "./identity.js";

/** Transport kind for an MCP server. */
export const McpTransportSchema = z.enum(["stdio", "sse"]);
export type McpTransport = z.infer<typeof McpTransportSchema>;

/** MCP server configuration (mirrors Rust `McpServerConfig`, camelCase). */
export const McpServerConfigSchema = z
  .object({
    command: z.string().min(1).max(1024).nullish(),
    args: z.array(z.string().max(1024)).max(32).default([]),
    env: z.record(z.string().min(1).max(128), z.string().max(4096)).default({}),
    url: z.string().max(2048).nullish(),
    transport: McpTransportSchema.default("stdio"),
    capabilities: z.array(z.string().min(1).max(128)).min(1).max(32),
    timeoutMs: z.number().int().min(100).max(300_000).nullish(),
  })
  .strict();
export type McpServerConfig = z.infer<typeof McpServerConfigSchema>;

/** MCP server status. */
export const McpServerStatusSchema = z.enum([
  "connected",
  "disconnected",
  "error",
]);
export type McpServerStatus = z.infer<typeof McpServerStatusSchema>;

/** Plugin trust state. */
export const PluginTrustSchema = z.enum(["pending", "trusted", "rejected"]);
export type PluginTrust = z.infer<typeof PluginTrustSchema>;

/** Configured MCP server with live status (mirrors Rust `McpServerInfo`). */
export const McpServerInfoSchema = z
  .object({
    name: z.string().min(1).max(128),
    transport: McpTransportSchema,
    capabilities: z.array(z.string().min(1).max(128)).max(32),
    timeoutMs: z.number().int().nonnegative(),
    status: McpServerStatusSchema,
    lastError: z.string().max(512).nullish(),
    trust: PluginTrustSchema,
  })
  .strict();
export type McpServerInfo = z.infer<typeof McpServerInfoSchema>;

/** MCP tool advertised by a server (mirrors Rust `McpToolDto`). */
export const McpToolSchema = z
  .object({
    name: z.string().min(1).max(128),
    description: z.string().max(4096),
    inputSchema: z.unknown(),
  })
  .strict();
export type McpTool = z.infer<typeof McpToolSchema>;

/** Receipt for one policy-gated MCP tool call. */
export const McpToolCallReceiptSchema = z
  .object({
    id: z.string().min(1).max(128),
    server: z.string().min(1).max(128),
    tool: z.string().min(1).max(128),
    taskId: z.string().min(1).max(128),
    decision: z.string().min(1).max(64),
    reason: z.string().max(2048),
    output: z.unknown(),
    isError: z.boolean(),
    evidenceId: z.string().min(1).max(128).nullish(),
  })
  .strict();
export type McpToolCallReceipt = z.infer<typeof McpToolCallReceiptSchema>;

/** Instruction file kinds. */
export const InstructionKindSchema = z.enum([
  "agents",
  "claude",
  "skill",
  "heartbeat",
]);
export type InstructionKind = z.infer<typeof InstructionKindSchema>;

/** Parsed instruction file summary (mirrors Rust `InstructionParseView`). */
export const InstructionParseViewSchema = z
  .object({
    path: z.string().min(1).max(1024),
    kind: InstructionKindSchema,
    summary: z.unknown(),
  })
  .strict();
export type InstructionParseView = z.infer<typeof InstructionParseViewSchema>;

/** Plugin manifest (mirrors Rust `PluginManifest`, camelCase). */
export const PluginManifestSchema = z
  .object({
    name: z.string().min(1).max(128),
    version: z.string().min(1).max(32),
    author: z.string().min(1).max(256),
    capabilities: z.array(z.string().min(1).max(128)).min(1).max(32),
    permissions: z.array(z.string().min(1).max(128)).max(32).default([]),
    sourceUrl: z.string().max(2048).nullish(),
    signature: z.string().length(128).nullish(),
  })
  .strict();
export type PluginManifest = z.infer<typeof PluginManifestSchema>;

/** Permission review payload shown before enabling a plugin. */
export const PermissionReviewSchema = z
  .object({
    plugin: z.string().min(1).max(256),
    requested: z.array(z.string().min(1).max(128)).max(32),
    capabilities: z.array(z.string().min(1).max(128)).max(32),
    unsigned: z.boolean(),
    signatureValid: z.boolean(),
  })
  .strict();
export type PermissionReview = z.infer<typeof PermissionReviewSchema>;

/** Installed plugin/skill with trust status (mirrors Rust `PluginDto`). */
export const PluginInfoSchema = z
  .object({
    name: z.string().min(1).max(128),
    version: z.string().min(1).max(32),
    author: z.string().max(256).nullish(),
    kind: z.enum(["mcp-server", "skill", "plugin"]),
    trust: PluginTrustSchema,
    capabilities: z.array(z.string().min(1).max(128)).max(32),
    permissions: z.array(z.string().min(1).max(128)).max(32),
    unsigned: z.boolean(),
  })
  .strict();
export type PluginInfo = z.infer<typeof PluginInfoSchema>;

/** Payload of `mcpServerConnected` (server + tool count, never secrets). */
export const McpServerConnectedPayloadSchema = z
  .object({
    server: z.string().min(1).max(128),
    toolCount: z.number().int().nonnegative(),
    checkedAt: IsoDateTimeSchema,
  })
  .strict();
export type McpServerConnectedPayload = z.infer<
  typeof McpServerConnectedPayloadSchema
>;

/** Payload of `mcpServerDisconnected` (server ref only). */
export const McpServerDisconnectedPayloadSchema = z
  .object({
    server: z.string().min(1).max(128),
    checkedAt: IsoDateTimeSchema,
  })
  .strict();
export type McpServerDisconnectedPayload = z.infer<
  typeof McpServerDisconnectedPayloadSchema
>;

/** Payload of `mcpToolCallCompleted` (refs + verdict, never arguments). */
export const McpToolCallCompletedPayloadSchema = z
  .object({
    server: z.string().min(1).max(128),
    tool: z.string().min(1).max(128),
    decision: z.string().min(1).max(64),
    checkedAt: IsoDateTimeSchema,
  })
  .strict();
export type McpToolCallCompletedPayload = z.infer<
  typeof McpToolCallCompletedPayloadSchema
>;

/** Payload of `instructionFileParsed` (path + kind only). */
export const InstructionFileParsedPayloadSchema = z
  .object({
    path: z.string().min(1).max(1024),
    kind: z.string().min(1).max(32),
    checkedAt: IsoDateTimeSchema,
  })
  .strict();
export type InstructionFileParsedPayload = z.infer<
  typeof InstructionFileParsedPayloadSchema
>;

/** Payload of `pluginApprovalRequested` (review, never secrets). */
export const PluginApprovalRequestedPayloadSchema = z
  .object({
    plugin: z.string().min(1).max(256),
    requested: z.array(z.string().min(1).max(128)).max(32),
    capabilities: z.array(z.string().min(1).max(128)).max(32),
    unsigned: z.boolean(),
    checkedAt: IsoDateTimeSchema,
  })
  .strict();
export type PluginApprovalRequestedPayload = z.infer<
  typeof PluginApprovalRequestedPayloadSchema
>;
