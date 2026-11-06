import { z } from "zod";

/**
 * HEARTBEAT scheduler contracts (Batch 5): shared read model for the
 * daemon-owned schedule set, mirroring `scheduler::ScheduleStatusView`
 * (Rust) exactly. The daemon parses HEARTBEAT.md, fires system-initiated
 * tasks, and answers `list_schedules` / `toggle_schedule_active` /
 * `trigger_schedule_now`; every layer renders these shapes.
 */

/** One owned schedule (stable id, cron display, firing state). */
export const ScheduleStatusViewSchema = z
  .object({
    id: z.string().min(1).max(128),
    name: z.string().min(1).max(128),
    expression: z.string().min(1).max(4096),
    cron: z.string().min(1).max(64),
    active: z.boolean(),
    nextTriggerAt: z.string().min(1),
    lastRunAt: z.string().min(1).nullish(),
    lastStatus: z.string().min(1).max(64).nullish(),
    runCount: z.number().int().nonnegative(),
  })
  .strict();
export type ScheduleStatusView = z.infer<typeof ScheduleStatusViewSchema>;

/** `list_schedules` request (no args; daemon owns the set). */
export const ListSchedulesSchema = z.object({}).strict();
export type ListSchedules = z.infer<typeof ListSchedulesSchema>;

/** `toggle_schedule_active` request (pause or resume one schedule). */
export const ToggleScheduleActiveSchema = z
  .object({
    scheduleId: z.string().min(1).max(128),
    active: z.boolean(),
  })
  .strict();
export type ToggleScheduleActive = z.infer<typeof ToggleScheduleActiveSchema>;

/** `trigger_schedule_now` request (manual fire, even when paused). */
export const TriggerScheduleNowSchema = z
  .object({ scheduleId: z.string().min(1).max(128) })
  .strict();
export type TriggerScheduleNow = z.infer<typeof TriggerScheduleNowSchema>;

/** Daemon `SchedulesListed` event payload (full set or one updated row). */
export const SchedulesListedPayloadSchema = z
  .object({
    schedules: z.array(ScheduleStatusViewSchema).max(32),
    inReplyTo: z.string().min(1).max(128),
  })
  .strict();
export type SchedulesListedPayload = z.infer<
  typeof SchedulesListedPayloadSchema
>;

/** Desktop notification for a schedule parked in `waiting_policy`. */
export const SchedulePolicyRequiredSchema = z
  .object({
    approvalId: z.string().min(1).max(128),
    taskId: z.string().min(1).max(128),
    summary: z.string().min(1).max(1024),
  })
  .strict();
export type SchedulePolicyRequired = z.infer<
  typeof SchedulePolicyRequiredSchema
>;

/** One daemon broadcast frame re-emitted by the Tauri shell. */
export const GatewayDaemonEventSchema = z
  .object({
    protocol: z.string().min(1).max(64),
    event: z.string().min(1).max(64),
    payload: z.unknown(),
    taskId: z.string().min(1).max(128).nullish(),
    at: z.string().min(1),
  })
  .strict();
export type GatewayDaemonEvent = z.infer<typeof GatewayDaemonEventSchema>;
