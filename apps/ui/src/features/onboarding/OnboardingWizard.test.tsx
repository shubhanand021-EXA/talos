import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { OnboardingWizard } from "./OnboardingWizard.js";
import type { Onboarding } from "./useOnboarding.js";

const {
  mockListProviders,
  mockStoreKey,
  mockTestConnection,
  mockRunDiagnostic,
} = vi.hoisted(() => ({
  mockListProviders: vi.fn(),
  mockStoreKey: vi.fn(),
  mockTestConnection: vi.fn(),
  mockRunDiagnostic: vi.fn(),
}));

vi.mock("../../bridge/index.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../bridge/index.js")>();
  return {
    ...actual,
    isTauri: () => true,
    listProviders: mockListProviders,
    storeProviderKey: mockStoreKey,
    testProviderConnection: mockTestConnection,
    runDiagnostic: mockRunDiagnostic,
  };
});

function stubOnboarding(overrides: Partial<Onboarding> = {}): Onboarding {
  return {
    loading: false,
    completed: false,
    complete: vi.fn().mockResolvedValue(undefined),
    reset: vi.fn().mockResolvedValue(undefined),
    workspaceDir: "",
    setWorkspaceDir: vi.fn(),
    ...overrides,
  };
}

function continueFlow(times: number): void {
  for (let i = 0; i < times; i += 1) {
    fireEvent.click(screen.getByRole("button", { name: "Next step" }));
  }
}

beforeEach(() => {
  vi.clearAllMocks();
  mockListProviders.mockResolvedValue([
    { providerId: "openai", configured: false },
    { providerId: "ollama", configured: false },
  ]);
  mockStoreKey.mockResolvedValue(true);
  mockTestConnection.mockResolvedValue({
    providerId: "openai",
    ok: true,
    latencyMs: 12,
    message: "reachable",
  });
  mockRunDiagnostic.mockResolvedValue({
    total: 3,
    passed: 3,
    checks: [],
    ledger: { healthy: true },
    task_id: "diag-1",
    agent_id: "agent-1",
    evaluation: { decision: "Allow" },
    claim_id: "c-1",
    plan_id: "p-1",
  });
});

describe("OnboardingWizard", () => {
  it("renders all six steps in order", async () => {
    render(<OnboardingWizard onboarding={stubOnboarding()} />);
    expect(
      screen.getByText("Talos OS — one policy, one ledger, every agent."),
    ).toBeInTheDocument();
    continueFlow(1);
    expect(
      await screen.findByText(/Add one LLM provider key/),
    ).toBeInTheDocument();
    continueFlow(1);
    expect(screen.getByLabelText("Workspace directory")).toBeInTheDocument();
    continueFlow(1);
    expect(screen.getByText("The safety contract.")).toBeInTheDocument();
    expect(
      screen.getByText(/Talos never acts without your permission/),
    ).toBeInTheDocument();
    continueFlow(1);
    expect(screen.getByText("Run demo task")).toBeInTheDocument();
    continueFlow(1);
    expect(
      screen.getByText("You are awake. What is Talos?"),
    ).toBeInTheDocument();
    expect(screen.getByText("Finish setup")).toBeInTheDocument();
  });

  it("walks back without losing the flow", async () => {
    render(<OnboardingWizard onboarding={stubOnboarding()} />);
    continueFlow(2);
    expect(screen.getByLabelText("Workspace directory")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Previous step" }));
    expect(
      await screen.findByText(/Add one LLM provider key/),
    ).toBeInTheDocument();
  });

  it("stores the provider key in the keychain (mock)", async () => {
    render(<OnboardingWizard onboarding={stubOnboarding()} />);
    continueFlow(1);
    await screen.findByText(/Add one LLM provider key/);
    const input = screen.getByLabelText("API key for openai");
    fireEvent.change(input, { target: { value: "sk-test" } });
    fireEvent.click(screen.getByRole("button", { name: "Save key" }));
    await waitFor(() => {
      expect(mockStoreKey).toHaveBeenCalledWith("openai", "sk-test");
    });
    expect(
      await screen.findByText(
        "Key stored for openai (OS keychain — never displayed).",
      ),
    ).toBeInTheDocument();
  });

  it("tests the provider connection", async () => {
    render(<OnboardingWizard onboarding={stubOnboarding()} />);
    continueFlow(1);
    await screen.findByText(/Add one LLM provider key/);
    fireEvent.click(screen.getByRole("button", { name: "Test connection" }));
    await waitFor(() => {
      expect(mockTestConnection).toHaveBeenCalledWith("openai");
    });
    expect(await screen.findByText(/reachable/)).toBeInTheDocument();
  });

  it("runs the first-task demo and reports completion", async () => {
    render(<OnboardingWizard onboarding={stubOnboarding()} />);
    continueFlow(4);
    expect(screen.getByText("not run yet")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Run demo task" }));
    expect(await screen.findByText("passed")).toBeInTheDocument();
    expect(
      await screen.findByText(/Diagnostic: 3\/3 checks passed/),
    ).toBeInTheDocument();
    expect(mockRunDiagnostic).toHaveBeenCalledTimes(1);
  });

  it("reports demo failures instead of advancing silently", async () => {
    mockRunDiagnostic.mockRejectedValueOnce(new Error("backend offline"));
    render(<OnboardingWizard onboarding={stubOnboarding()} />);
    continueFlow(4);
    fireEvent.click(screen.getByRole("button", { name: "Run demo task" }));
    expect(await screen.findByText("failed")).toBeInTheDocument();
    expect(await screen.findByText("backend offline")).toBeInTheDocument();
  });

  it("completes onboarding and persists the flag", async () => {
    const onboarding = stubOnboarding();
    render(<OnboardingWizard onboarding={onboarding} />);
    continueFlow(5);
    fireEvent.click(screen.getByRole("button", { name: "Finish setup" }));
    await waitFor(() => {
      expect(onboarding.complete).toHaveBeenCalledTimes(1);
    });
  });

  it("announces steps to screen readers", async () => {
    render(<OnboardingWizard onboarding={stubOnboarding()} />);
    expect(screen.getByRole("dialog")).toHaveAttribute(
      "aria-labelledby",
      "onboarding-title",
    );
    expect(screen.getByLabelText("Onboarding progress")).toBeInTheDocument();
    continueFlow(1);
    await screen.findByText(/Add one LLM provider key/);
  });
});
