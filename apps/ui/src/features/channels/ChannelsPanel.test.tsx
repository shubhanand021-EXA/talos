import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ChannelsPanel } from "./ChannelsPanel.js";

const { mockList, mockLink, mockUnlink, mockMessages } = vi.hoisted(() => ({
  mockList: vi.fn(),
  mockLink: vi.fn(),
  mockUnlink: vi.fn(),
  mockMessages: vi.fn(),
}));

vi.mock("../../bridge/gateway.js", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("../../bridge/gateway.js")>();
  return {
    ...actual,
    listChannels: mockList,
    linkChannel: mockLink,
    unlinkChannel: mockUnlink,
    listChannelMessages: mockMessages,
  };
});

vi.mock("../../bridge/client.js", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("../../bridge/client.js")>();
  return { ...actual, isTauri: () => true };
});

const ROWS = [
  {
    channel: "telegram",
    enabled: true,
    linked: true,
    lastPollAt: null,
    messageCount: 2,
  },
  {
    channel: "discord",
    enabled: false,
    linked: false,
    lastPollAt: null,
    messageCount: 0,
  },
];

beforeEach(() => {
  vi.clearAllMocks();
  mockList.mockResolvedValue(ROWS);
  mockMessages.mockResolvedValue([
    {
      channel: "telegram",
      direction: "in",
      chatId: "chat-9",
      excerpt: "summarize the ledger",
      at: "2026-11-06T10:00:00Z",
    },
  ]);
  mockLink.mockImplementation((channel: string) =>
    Promise.resolve(
      ROWS.map((row) =>
        row.channel === channel ? { ...row, enabled: true, linked: true } : row,
      ),
    ),
  );
  mockUnlink.mockImplementation((channel: string) =>
    Promise.resolve(
      ROWS.map((row) =>
        row.channel === channel
          ? { ...row, enabled: false, linked: false }
          : row,
      ),
    ),
  );
});

describe("ChannelsPanel", () => {
  it("lists link state and recent traffic", async () => {
    render(<ChannelsPanel />);
    expect(await screen.findByText("telegram")).toBeInTheDocument();
    expect(screen.getByText("discord")).toBeInTheDocument();
    expect(screen.getByText("linked")).toBeInTheDocument();
    expect(screen.getByText("unlinked")).toBeInTheDocument();
    expect(screen.getByText(/summarize the ledger/)).toBeInTheDocument();
    expect(mockList).toHaveBeenCalledTimes(1);
  });

  it("links a bot with a pasted token", async () => {
    render(<ChannelsPanel />);
    const input = await screen.findByLabelText("discord token or webhook URL");
    fireEvent.change(input, { target: { value: "WEBHOOK-URL" } });
    fireEvent.click(screen.getByRole("button", { name: "Link bot" }));
    await waitFor(() => {
      expect(mockLink).toHaveBeenCalledWith("discord", "WEBHOOK-URL");
    });
  });

  it("unlinks a channel", async () => {
    render(<ChannelsPanel />);
    fireEvent.click(await screen.findByRole("button", { name: "Unlink" }));
    await waitFor(() => {
      expect(mockUnlink).toHaveBeenCalledWith("telegram");
    });
  });
});
