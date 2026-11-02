import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { mockInvoke } = vi.hoisted(() => ({
  mockInvoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: mockInvoke }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

import { SkillManager } from "./SkillManager.js";
import { useAppStore } from "../../stores/appStore.js";

const NOW = "2026-10-31T00:00:00.000Z";

function skillRow(
  id: string,
  status: string,
  extra: Record<string, unknown> = {},
) {
  return {
    id,
    name: `skill ${id}`,
    description: `Procedure ${id}.`,
    procedureText: "1. Call `fs.read`. 2. Record the hash.",
    version: "1",
    status,
    sourceTaskId: "task-1",
    testFixtureRef: "task-1",
    permissionCeiling: "low",
    requiredTools: ["fs.read"],
    usageCount: 5,
    successCount: 5,
    failureCount: 0,
    confidenceScore: 1,
    lastUsedAt: NOW,
    parentVersion: null,
    createdAt: NOW,
    ...extra,
  };
}

const SKILLS = [
  skillRow("skill-q", "quarantined", {
    usageCount: 0,
    successCount: 0,
    confidenceScore: 0,
    lastUsedAt: null,
  }),
  skillRow("skill-v", "validated"),
  skillRow("skill-p", "published"),
  skillRow("skill-d", "deprecated"),
  skillRow("skill-a", "archived"),
];

function renderManager(skills: unknown[] = SKILLS) {
  return render(
    <MemoryRouter>
      <SkillManager
        skills={skills as never}
        onRefresh={() => Promise.resolve()}
      />
    </MemoryRouter>,
  );
}

beforeEach(() => {
  vi.unstubAllGlobals();
  mockInvoke.mockReset();
  useAppStore.setState({ liveFeed: [], evidenceVersion: 0 });
  vi.stubGlobal("__TAURI_INTERNALS__", {});
  mockInvoke.mockImplementation(
    (command: string, args?: Record<string, unknown>) => {
      if (command === "get_skill") {
        const row = SKILLS.find((skill) => skill.id === args?.["skill_id"]);
        return Promise.resolve(row);
      }
      if (command === "get_skill_stats") {
        return Promise.resolve({
          skillId: args?.["skill_id"],
          usageCount: 5,
          successCount: 5,
          failureCount: 0,
          successRate: 1,
          confidenceScore: 1,
          lastUsedAt: NOW,
          needsReview: false,
        });
      }
      if (command === "list_skill_versions") {
        const row = SKILLS.find((skill) => skill.id === args?.["skill_id"]);
        return Promise.resolve([
          row,
          {
            ...(row as object),
            id: `${(row as { id: string }).id}-v2`,
            version: "2",
          },
        ]);
      }
      if (command === "diff_skill_versions") {
        return Promise.resolve([
          { kind: "same", text: "1. Call `fs.read`." },
          { kind: "removed", text: "2. Record the hash." },
          { kind: "added", text: "2. Record the hash twice." },
        ]);
      }
      if (
        command === "validate_skill" ||
        command === "publish_skill" ||
        command === "deprecate_skill" ||
        command === "archive_skill"
      ) {
        const row = SKILLS.find((skill) => skill.id === args?.["skill_id"]);
        const next =
          command === "validate_skill"
            ? "validated"
            : command === "publish_skill"
              ? "published"
              : command === "deprecate_skill"
                ? "deprecated"
                : "archived";
        return Promise.resolve({ ...(row as object), status: next });
      }
      if (command === "trigger_skill_evaluation") {
        return Promise.resolve({ started: true, due: true, receiptCount: 30 });
      }
      if (command === "search_memory") {
        return Promise.resolve([
          {
            id: "skill:skill-p",
            sourceType: "skill",
            sourceId: "skill-p",
            taskId: null,
            score: 0.91,
            lexicalScore: 0.8,
            semanticScore: 0.95,
            snippet: "skill read manifest",
            verified: true,
            createdAt: NOW,
            reason: "hybrid",
          },
        ]);
      }
      return Promise.reject(new Error(`unexpected ${command}`));
    },
  );
});

describe("SkillManager", () => {
  it("renders every lifecycle status", async () => {
    renderManager();
    for (const status of [
      "quarantined",
      "validated",
      "published",
      "deprecated",
      "archived",
    ]) {
      expect(await screen.findByText(status)).toBeInTheDocument();
    }
  });

  it("shows usage statistics in the registry", async () => {
    renderManager();
    await screen.findByText("skill skill-q");
    expect(screen.getAllByText("5").length).toBeGreaterThan(0);
    expect(screen.getAllByText("100%").length).toBeGreaterThan(0);
  });

  it("opens the document viewer and runs lifecycle actions", async () => {
    const user = userEvent.setup();
    renderManager();
    await user.click(await screen.findByText("skill skill-q"));
    expect(await screen.findByLabelText("Skill document")).toHaveTextContent(
      "1. Call `fs.read`. 2. Record the hash.",
    );
    expect(screen.getByText("5 uses", { exact: false })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Validate" }));
    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith(
        "validate_skill",
        expect.objectContaining({ skill_id: "skill-q" }),
      );
    });
  });

  it("shows version history with a diff view", async () => {
    const user = userEvent.setup();
    renderManager();
    await user.click(await screen.findByText("skill skill-p"));
    expect(await screen.findByText(/v1 → v2/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Show diff" }));
    expect(await screen.findByLabelText("Version diff")).toBeInTheDocument();
    expect(
      screen.getByText("2. Record the hash twice.", { exact: false }),
    ).toBeInTheDocument();
  });

  it("triggers the GEPA evaluation loop", async () => {
    const user = userEvent.setup();
    renderManager();
    await user.click(screen.getByRole("button", { name: "Evaluate Now" }));
    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("trigger_skill_evaluation", {
        every_n_calls: null,
      });
    });
    expect(
      await screen.findByText("GEPA evaluation started", { exact: false }),
    ).toBeInTheDocument();
  });
});
