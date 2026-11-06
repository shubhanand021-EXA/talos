import { z } from "zod";

/**
 * Messaging channel contracts (Batch 5): shared read model for the
 * daemon-owned Telegram / Discord surfaces. Secrets (bot tokens, webhook
 * URLs) never cross this boundary — the UI sends them to `link_channel`
 * once (keychain-bound) and only ever reads back linked/enabled flags.
 */

/** Supported external channels. */
export const ChannelIdSchema = z.enum(["telegram", "discord"]);
export type ChannelId = z.infer<typeof ChannelIdSchema>;

/** One channel status row (never carries tokens). */
export const ChannelStatusViewSchema = z
  .object({
    channel: ChannelIdSchema,
    enabled: z.boolean(),
    linked: z.boolean(),
    lastPollAt: z.string().min(1).nullish(),
    messageCount: z.number().int().nonnegative(),
  })
  .strict();
export type ChannelStatusView = z.infer<typeof ChannelStatusViewSchema>;

/** One logged message row (metadata + excerpt, never secrets). */
export const LoggedMessageSchema = z
  .object({
    channel: ChannelIdSchema,
    direction: z.enum(["in", "out"]),
    chatId: z.string().min(1).max(128),
    excerpt: z.string().max(256),
    at: z.string().min(1),
  })
  .strict();
export type LoggedMessage = z.infer<typeof LoggedMessageSchema>;

/** One inbound user message (normalized across adapters). */
export const IncomingMessageSchema = z
  .object({
    channel: ChannelIdSchema,
    messageId: z.string().min(1).max(128),
    senderId: z.string().min(1).max(128),
    chatId: z.string().min(1).max(128),
    text: z.string().min(1).max(4096),
    receivedAt: z.string().min(1),
  })
  .strict();
export type IncomingMessage = z.infer<typeof IncomingMessageSchema>;

/** One outbound reply queued for an adapter. */
export const OutgoingMessageSchema = z
  .object({
    chatId: z.string().min(1).max(128),
    text: z.string().min(1).max(4000),
  })
  .strict();
export type OutgoingMessage = z.infer<typeof OutgoingMessageSchema>;

/** `link_channel` request: token travels once into the keychain. */
export const LinkChannelSchema = z
  .object({
    channel: ChannelIdSchema,
    token: z.string().min(1).max(512),
  })
  .strict();
export type LinkChannel = z.infer<typeof LinkChannelSchema>;

/** `unlink_channel` request (idempotent). */
export const UnlinkChannelSchema = z
  .object({ channel: ChannelIdSchema })
  .strict();
export type UnlinkChannel = z.infer<typeof UnlinkChannelSchema>;

/** Secret-free channel configuration (mirrors `.talos/channels.json`). */
export const ChannelConfigSchema = z
  .object({
    enabled: z.boolean(),
    allowedChatIds: z.array(z.string().min(1).max(128)).max(1024).default([]),
    pollIntervalSecs: z.number().int().min(5).max(3600).default(30),
  })
  .strict();
export type ChannelConfig = z.infer<typeof ChannelConfigSchema>;
