import { describe, expect, it } from "vitest";
import {
  GatewayDaemonEventSchema,
  ListSchedulesSchema,
  ScheduleStatusViewSchema,
  SchedulesListedPayloadSchema,
  ToggleScheduleActiveSchema,
  TriggerScheduleNowSchema,
} from "./scheduler.js";

const VIEW = {
  id: "sched-0-standup",
  name: "standup",
  expression: "every morning at 9am",
  cron: "0 9 * * *",
  active: true,
  nextTriggerAt: "2026-11-07T09:00:00Z",
  lastRunAt: null,
  lastStatus: "completed",
  runCount: 3,
};

describe("scheduler contracts", () => {
  it("validates the Heartbeats panel read model", () => {
    expect(ScheduleStatusViewSchema.safeParse(VIEW).success).toBe(true);
  });

  it("rejects empty schedule ids", () => {
    expect(
      ToggleScheduleActiveSchema.safeParse({ scheduleId: "", active: true })
        .success,
    ).toBe(false);
    expect(TriggerScheduleNowSchema.safeParse({ scheduleId: "" }).success).toBe(
      false,
    );
  });

  it("validates SchedulesListed daemon replies", () => {
    expect(
      SchedulesListedPayloadSchema.safeParse({
        schedules: [VIEW],
        inReplyTo: "tauri-1",
      }).success,
    ).toBe(true);
  });

  it("validates daemon broadcast frames re-emitted by the shell", () => {
    expect(
      GatewayDaemonEventSchema.safeParse({
        protocol: "talos-gateway/1",
        event: "PolicyRequired",
        payload: {
          approvalId: "sched-approval-t1",
          taskId: "t1",
          summary: "needs review",
        },
        taskId: "t1",
        at: new Date().toISOString(),
      }).success,
    ).toBe(true);
  });

  it("accepts empty list_schedules requests", () => {
    expect(ListSchedulesSchema.safeParse({}).success).toBe(true);
  });
});
