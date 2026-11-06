import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { RunnersPanel } from "./RunnersPanel.js";

const { mockList, mockAdd, mockRemove, mockTest } = vi.hoisted(() => ({
  mockList: vi.fn(),
  mockAdd: vi.fn(),
  mockRemove: vi.fn(),
  mockTest: vi.fn(),
}));

vi.mock("../../bridge/gateway.js", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("../../bridge/gateway.js")>();
  return {
    ...actual,
    listRunners: mockList,
    addRunner: mockAdd,
    removeRunner: mockRemove,
    testRunner: mockTest,
  };
});

vi.mock("../../bridge/client.js", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("../../bridge/client.js")>();
  return { ...actual, isTauri: () => true };
});

const ROWS = [
  {
    id: "gpu-box",
    kind: "ssh",
    enabled: true,
    lastTestOk: null,
    lastTestMessage: null,
  },
];

beforeEach(() => {
  vi.clearAllMocks();
  mockList.mockResolvedValue(ROWS);
  mockAdd.mockImplementation((runner: { id: string }) =>
    Promise.resolve([
      {
        id: runner.id,
        kind: "ssh",
        enabled: true,
        lastTestOk: null,
        lastTestMessage: null,
      },
    ]),
  );
  mockRemove.mockResolvedValue([]);
  mockTest.mockResolvedValue({
    id: "gpu-box",
    kind: "ssh",
    enabled: true,
    lastTestOk: true,
    lastTestMessage: "reachable",
  });
});

describe("RunnersPanel", () => {
  it("lists runners with kind and status", async () => {
    render(<RunnersPanel />);
    expect(await screen.findByText("gpu-box")).toBeInTheDocument();
    expect(screen.getByText("ssh")).toBeInTheDocument();
    expect(screen.getByText("enabled")).toBeInTheDocument();
    expect(mockList).toHaveBeenCalledTimes(1);
  });

  it("adds an SSH runner from the form", async () => {
    render(<RunnersPanel />);
    await screen.findByText("gpu-box");
    fireEvent.change(screen.getByLabelText("Runner id"), {
      target: { value: "gpu-2" },
    });
    fireEvent.change(screen.getByLabelText("SSH host"), {
      target: { value: "gpu-2.local" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add runner" }));
    await waitFor(() => {
      expect(mockAdd).toHaveBeenCalledWith(
        expect.objectContaining({
          id: "gpu-2",
          kind: "ssh",
          host: "gpu-2.local",
          enabled: true,
        }),
      );
    });
  });

  it("tests a connection and shows reachability", async () => {
    render(<RunnersPanel />);
    fireEvent.click(
      await screen.findByRole("button", { name: "Test connection" }),
    );
    await waitFor(() => {
      expect(mockTest).toHaveBeenCalledWith("gpu-box");
    });
    expect(await screen.findByText("reachable")).toBeInTheDocument();
  });

  it("removes a runner", async () => {
    render(<RunnersPanel />);
    fireEvent.click(await screen.findByRole("button", { name: "Remove" }));
    await waitFor(() => {
      expect(mockRemove).toHaveBeenCalledWith("gpu-box");
    });
  });
});
