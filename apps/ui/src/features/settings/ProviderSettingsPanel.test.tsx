import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ProvidersSection } from "./ProviderSettingsPanel.js";

const {
  mockListProviders,
  mockListModels,
  mockTest,
  mockUsage,
  mockStore,
  mockDelete,
} = vi.hoisted(() => ({
  mockListProviders: vi.fn(),
  mockListModels: vi.fn(),
  mockTest: vi.fn(),
  mockUsage: vi.fn(),
  mockStore: vi.fn(),
  mockDelete: vi.fn(),
}));

vi.mock("../../bridge/index.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../bridge/index.js")>();
  return {
    ...actual,
    listProviders: mockListProviders,
    listModels: mockListModels,
    testProviderConnection: mockTest,
    getUsageSummary: mockUsage,
    storeProviderKey: mockStore,
    deleteProviderKey: mockDelete,
    subscribeTalosEvent: vi.fn().mockResolvedValue(() => undefined),
  };
});

const PROVIDERS = [
  {
    providerId: "openai",
    configured: false,
    reachable: null,
    latencyMs: null,
    models: ["gpt-4o-mini"],
  },
  {
    providerId: "ollama",
    configured: true,
    reachable: null,
    latencyMs: null,
    models: ["llama3"],
  },
];

beforeEach(() => {
  vi.clearAllMocks();
  mockListProviders.mockResolvedValue(PROVIDERS);
  mockListModels.mockResolvedValue([
    {
      providerId: "openai",
      modelId: "gpt-4o-mini",
      contextWindow: 128000,
      supportsTools: true,
      supportsVision: true,
      supportsStreaming: true,
      costPer1kInput: 0.00015,
      costPer1kOutput: 0.0006,
    },
  ]);
  mockUsage.mockResolvedValue({
    totalTokens: 15,
    estimatedCostUsd: 0.0000045,
    callCount: 1,
    byProvider: [],
  });
  mockTest.mockResolvedValue({
    providerId: "openai",
    ok: true,
    latencyMs: 120,
    message: "served",
  });
  mockStore.mockResolvedValue(true);
  mockDelete.mockResolvedValue(true);
});

describe("ProvidersSection", () => {
  it("renders providers with key and reachability status", async () => {
    render(<ProvidersSection />);
    expect(await screen.findByText("Model providers")).toBeTruthy();
    expect(await screen.findByText("openai")).toBeTruthy();
    expect(await screen.findByText("ollama")).toBeTruthy();
    expect(screen.getByText("no key")).toBeTruthy();
    expect(screen.getByText("key stored")).toBeTruthy();
  });

  it("runs the add-key flow without echoing the value", async () => {
    render(<ProvidersSection />);
    await screen.findByText("openai");
    const input = screen.getByLabelText("API key for openai");
    fireEvent.change(input, { target: { value: "sk-test-secret" } });
    const addButtons = screen.getAllByText("Add key");
    expect(addButtons.length).toBeGreaterThan(0);
    fireEvent.click(addButtons[0]!);
    await waitFor(() => {
      expect(mockStore).toHaveBeenCalledWith("openai", "sk-test-secret");
    });
    // The key value is cleared and never rendered as text.
    expect(document.body.textContent).not.toContain("sk-test-secret");
    expect((input as HTMLInputElement).value).toBe("");
    expect((input as HTMLInputElement).type).toBe("password");
  });

  it("displays test connection results and usage summary", async () => {
    render(<ProvidersSection />);
    await screen.findByText("openai");
    const testButtons = screen.getAllByText("Test connection");
    expect(testButtons.length).toBeGreaterThan(0);
    fireEvent.click(testButtons[0]!);
    await waitFor(() => {
      expect(mockTest).toHaveBeenCalledWith("openai");
    });
    expect(await screen.findByText(/reachable — served/)).toBeTruthy();
    expect(await screen.findByText("totalTokens")).toBeTruthy();
  });

  it("removes keys through the delete action", async () => {
    render(<ProvidersSection />);
    await screen.findByText("ollama");
    const removeButtons = screen.getAllByText("Remove key");
    expect(removeButtons.length).toBeGreaterThan(1);
    fireEvent.click(removeButtons[1]!);
    await waitFor(() => {
      expect(mockDelete).toHaveBeenCalledWith("ollama");
    });
  });
});
