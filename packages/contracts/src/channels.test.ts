import { describe, expect, it } from "vitest";
import {
  ChannelConfigSchema,
  ChannelStatusViewSchema,
  IncomingMessageSchema,
  LinkChannelSchema,
  LoggedMessageSchema,
  OutgoingMessageSchema,
  UnlinkChannelSchema,
} from "./channels.js";

describe("channel contracts", () => {
  it("validates the Channels tab read models", () => {
    expect(
      ChannelStatusViewSchema.safeParse({
        channel: "telegram",
        enabled: true,
        linked: true,
        lastPollAt: null,
        messageCount: 3,
      }).success,
    ).toBe(true);
    expect(
      LoggedMessageSchema.safeParse({
        channel: "discord",
        direction: "in",
        chatId: "chan-1",
        excerpt: "hello",
        at: new Date().toISOString(),
      }).success,
    ).toBe(true);
  });

  it("validates inbound and outbound messages", () => {
    expect(
      IncomingMessageSchema.safeParse({
        channel: "telegram",
        messageId: "m-1",
        senderId: "u-1",
        chatId: "c-9",
        text: "summarize the ledger",
        receivedAt: new Date().toISOString(),
      }).success,
    ).toBe(true);
    expect(
      OutgoingMessageSchema.safeParse({ chatId: "c-9", text: "done" }).success,
    ).toBe(true);
    expect(
      IncomingMessageSchema.safeParse({
        channel: "telegram",
        messageId: "m-1",
        senderId: "u-1",
        chatId: "c-9",
        text: "",
        receivedAt: new Date().toISOString(),
      }).success,
    ).toBe(false);
  });

  it("validates link and unlink requests", () => {
    expect(
      LinkChannelSchema.safeParse({ channel: "telegram", token: "BOT" })
        .success,
    ).toBe(true);
    expect(
      LinkChannelSchema.safeParse({ channel: "sms", token: "BOT" }).success,
    ).toBe(false);
    expect(UnlinkChannelSchema.safeParse({ channel: "discord" }).success).toBe(
      true,
    );
  });

  it("validates secret-free channel config", () => {
    const parsed = ChannelConfigSchema.safeParse({
      enabled: true,
      allowedChatIds: ["777"],
    });
    expect(parsed.success).toBe(true);
    if (parsed.success) {
      expect(parsed.data.pollIntervalSecs).toBe(30);
    }
  });
});
