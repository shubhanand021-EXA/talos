import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { HeartbeatsPanel } from "./HeartbeatsPanel.js";

const { mockList, mockToggle, mockTrigger } = vi.hoisted(() => ({
  mockList: vi.fn(),
  mockToggle: vi.fn(),
  mockTrigger: vi.fn(),
}));

vi.mock("../../bridge/gateway.js", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("../../bridge/gateway.js")>();
  return {
    ...actual,
    listSchedules: mockList,
    toggleSchedule: mockToggle,
    triggerScheduleNow: mockTrigger,
  };
});

vi.mock("../../bridge/client.js", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("../../bridge/client.js")>();
  return { ...actual, isTauri: () => true };
});

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => undefined),
}));

const SCHEDULES = [
  {
    id: "sched-0-standup",
    name: "standup",
    expression: "every morning at 9am",
    cron: "0 9 * * *",
    active: true,
    nextTriggerAt: "2026-11-07T09:00:00Z",
    lastRunAt: "2026-11-06T09:00:00Z",
    lastStatus: "completed",
    runCount: 3,
  },
  {
    id: "sched-1-bills",
    name: "bills",
    expression: "monthly on the 15th at 10:00",
    cron: "0 10 15 * *",
    active: false,
    nextTriggerAt: "2026-11-15T10:00:00Z",
    lastRunAt: null,
    lastStatus: null,
    runCount: 0,
  },
];

beforeEach(() => {
  vi.clearAllMocks();
  mockList.mockResolvedValue(SCHEDULES);
  mockToggle.mockImplementation((id: string, active: boolean) =>
    Promise.resolve([
      { ...(SCHEDULES[0] as Record<string, unknown>), id, active },
    ]),
  );
  mockTrigger.mockResolvedValue({ taskId: "task-1", status: "completed" });
});

describe("HeartbeatsPanel", () => {
  it("lists schedules with next trigger and last status", async () => {
    render(<HeartbeatsPanel />);
    expect(await screen.findByText("standup")).toBeInTheDocument();
    expect(screen.getByText("bills")).toBeInTheDocument();
    expect(screen.getByText("0 9 * * * UTC")).toBeInTheDocument();
    expect(
      screen.getByText(/next 2026-11-07T09:00:00Z · last completed/),
    ).toBeInTheDocument();
    expect(mockList).toHaveBeenCalledTimes(1);
  });

  it("pauses an active schedule", async () => {
    render(<HeartbeatsPanel />);
    const pause = await screen.findByRole("button", { name: "Pause" });
    fireEvent.click(pause);
    await waitFor(() => {
      expect(mockToggle).toHaveBeenCalledWith("sched-0-standup", false);
    });
  });

  it("triggers a schedule now and refreshes", async () => {
    render(<HeartbeatsPanel />);
    const triggers = await screen.findAllByRole("button", {
      name: "Trigger now",
    });
    fireEvent.click(triggers[0] as HTMLElement);
    await waitFor(() => {
      expect(mockTrigger).toHaveBeenCalledWith("sched-0-standup");
    });
    await waitFor(() => {
      expect(mockList).toHaveBeenCalledTimes(2);
    });
  });
});
