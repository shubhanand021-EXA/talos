import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { McpPage } from "./McpPage.js";

const {
  mockListMcpServers,
  mockListPlugins,
  mockAddMcpServer,
  mockRemoveMcpServer,
  mockApprovePlugin,
} = vi.hoisted(() => ({
  mockListMcpServers: vi.fn(),
  mockListPlugins: vi.fn(),
  mockAddMcpServer: vi.fn(),
  mockRemoveMcpServer: vi.fn(),
  mockApprovePlugin: vi.fn(),
}));

vi.mock("../../bridge/index.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../bridge/index.js")>();
  return {
    ...actual,
    listMcpServers: mockListMcpServers,
    listPlugins: mockListPlugins,
    addMcpServer: mockAddMcpServer,
    removeMcpServer: mockRemoveMcpServer,
    approvePlugin: mockApprovePlugin,
  };
});

const SERVERS = [
  {
    name: "fs",
    transport: "stdio",
    capabilities: ["tools"],
    timeoutMs: 30000,
    status: "disconnected",
    trust: "trusted",
  },
];

const PLUGINS = [
  {
    name: "fs-pack",
    version: "0.1.0",
    author: "talos",
    kind: "plugin",
    trust: "pending",
    capabilities: ["tools"],
    permissions: ["fs.read"],
    unsigned: true,
  },
];

beforeEach(() => {
  mockListMcpServers.mockReset();
  mockListPlugins.mockReset();
  mockAddMcpServer.mockReset();
  mockRemoveMcpServer.mockReset();
  mockApprovePlugin.mockReset();
  mockListMcpServers.mockResolvedValue(SERVERS);
  mockListPlugins.mockResolvedValue(PLUGINS);
  mockAddMcpServer.mockResolvedValue({ server: SERVERS[0], review: null });
  mockRemoveMcpServer.mockResolvedValue(true);
  mockApprovePlugin.mockImplementation((name: string, approved: boolean) =>
    Promise.resolve({
      ...PLUGINS[0],
      name,
      trust: approved ? "trusted" : "rejected",
    }),
  );
});

describe("McpPage", () => {
  it("renders the server list", async () => {
    render(<McpPage />);
    expect(await screen.findByText("fs")).toBeInTheDocument();
    expect(screen.getByText("stdio")).toBeInTheDocument();
  });

  it("adds and removes a server", async () => {
    const user = userEvent.setup();
    render(<McpPage />);
    await screen.findByText("fs");
    await user.click(screen.getByRole("button", { name: "Add server" }));
    await waitFor(() => {
      expect(mockAddMcpServer).toHaveBeenCalled();
    });
    const removeButtons = await screen.findAllByRole("button", {
      name: "Remove",
    });
    await user.click(removeButtons[0] as Element);
    await waitFor(() => {
      expect(mockRemoveMcpServer).toHaveBeenCalledWith("fs");
    });
  });

  it("renders the permission dialog when a review is returned", async () => {
    const user = userEvent.setup();
    mockAddMcpServer.mockResolvedValue({
      server: SERVERS[0],
      review: {
        plugin: "fs-pack 0.1.0",
        requested: ["fs.read"],
        capabilities: ["tools"],
        unsigned: true,
        signatureValid: false,
      },
    });
    render(<McpPage />);
    await screen.findByText("fs");
    await user.click(screen.getByRole("button", { name: "Add server" }));
    expect(
      await screen.findByText("Permission review: fs-pack 0.1.0"),
    ).toBeInTheDocument();
    expect(screen.getByText(/Unsigned plugin/)).toBeInTheDocument();
  });
});
