//! Scheduler engine: owned schedules, active flags, and a tick loop.
//!
//! The engine never executes tasks itself: [`Scheduler::tick_once`] returns
//! the schedules due at `now`, and the caller (the gateway daemon) submits
//! each one into the Supervisor as a system-initiated task. This keeps time
//! math unit-testable without a database and keeps policy-gated execution in
//! exactly one place (the gateway).

use chrono::{DateTime, Utc};

/// One owned schedule: parsed definition + runtime bookkeeping.
#[derive(Debug, Clone)]
pub struct RuntimeSchedule {
    /// Stable id (`sched-{index}-{slug}`).
    pub id: String,
    /// Human name from the HEARTBEAT.md heading.
    pub name: String,
    /// Original natural-language expression (verbatim).
    pub expression: String,
    /// Five-field cron rendering (UTC) for display/export.
    pub cron: String,
    /// Firing rule.
    pub trigger: crate::Trigger,
    /// Goal handed to the Supervisor on fire (the task payload).
    pub goal: String,
    /// Paused schedules never fire.
    pub active: bool,
    /// Last firing instant, if any.
    pub last_run: Option<DateTime<Utc>>,
    /// Next firing instant (recomputed after every tick/fire).
    pub next_trigger: DateTime<Utc>,
    /// Outcome of the last fire (`completed`, `failed`, `waiting_policy`, ...).
    pub last_status: Option<String>,
    /// Total fires so far.
    pub run_count: u64,
}

/// Public read model for the UI Heartbeats panel (all fields renderable).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleStatusView {
    /// Stable schedule id.
    pub id: String,
    /// Human name.
    pub name: String,
    /// Original NL expression.
    pub expression: String,
    /// Cron rendering (UTC).
    pub cron: String,
    /// Whether the schedule fires.
    pub active: bool,
    /// RFC 3339 next firing time (UTC).
    pub next_trigger_at: String,
    /// RFC 3339 last firing time, if any.
    pub last_run_at: Option<String>,
    /// Outcome of the last fire, if any.
    pub last_status: Option<String>,
    /// Total fires.
    pub run_count: u64,
}

/// Owned schedule set with deterministic ticking.
#[derive(Debug, Default)]
pub struct Scheduler {
    schedules: Vec<RuntimeSchedule>,
}

impl Scheduler {
    /// Empty scheduler (tests, or a missing HEARTBEAT.md).
    #[must_use]
    pub fn new() -> Self {
        Self {
            schedules: Vec::new(),
        }
    }

    /// Load schedules from parsed HEARTBEAT.md tasks, anchoring every
    /// `next_trigger` at `now`. Tasks without a usable trigger are skipped
    /// (logged by the caller), never fatal.
    #[must_use]
    pub fn from_tasks(tasks: &[instruction_parser::ScheduledTask], now: DateTime<Utc>) -> Self {
        let mut scheduler = Self::new();
        for (index, task) in tasks.iter().enumerate() {
            let Some(trigger) = crate::trigger_for(task) else {
                continue;
            };
            let slug: String = task
                .name
                .to_ascii_lowercase()
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                .collect::<String>()
                .split('-')
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join("-");
            let slug = if slug.is_empty() {
                "task".to_owned()
            } else {
                slug
            };
            let next_trigger = trigger.next_after(now, None);
            scheduler.schedules.push(RuntimeSchedule {
                id: format!("sched-{index}-{slug}"),
                name: task.name.clone(),
                expression: task.expression.clone(),
                cron: trigger.to_cron(),
                trigger,
                goal: if task.payload.trim().is_empty() {
                    task.name.clone()
                } else {
                    task.payload.clone()
                },
                active: true,
                last_run: None,
                next_trigger,
                last_status: None,
                run_count: 0,
            });
        }
        scheduler
    }

    /// Load HEARTBEAT.md from `path` (missing file ⇒ empty scheduler).
    pub fn load_file(path: &std::path::Path, now: DateTime<Utc>) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::new();
        };
        match instruction_parser::parse_heartbeat(&text, Some(path.display().to_string())) {
            Ok(tasks) => Self::from_tasks(&tasks, now),
            Err(_) => Self::new(),
        }
    }

    /// Number of owned schedules.
    #[must_use]
    pub fn len(&self) -> usize {
        self.schedules.len()
    }

    /// True when no schedules are owned.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.schedules.is_empty()
    }

    /// Read model for the UI (sorted by next trigger).
    #[must_use]
    pub fn status(&self) -> Vec<ScheduleStatusView> {
        let mut views: Vec<ScheduleStatusView> = self
            .schedules
            .iter()
            .map(ScheduleStatusView::from)
            .collect();
        views.sort_by(|a, b| a.next_trigger_at.cmp(&b.next_trigger_at));
        views
    }

    /// Flip one schedule's active flag. Returns the updated view, or `None`
    /// for an unknown id. Reactivating recomputes the next trigger from now.
    pub fn set_active(
        &mut self,
        id: &str,
        active: bool,
        now: DateTime<Utc>,
    ) -> Option<ScheduleStatusView> {
        let schedule = self.schedules.iter_mut().find(|s| s.id == id)?;
        schedule.active = active;
        if active {
            schedule.next_trigger = schedule.trigger.next_after(now, schedule.last_run);
        }
        Some(ScheduleStatusView::from(&*schedule))
    }

    /// Schedules due at `now` (active and past their trigger). Advances every
    /// due schedule's `next_trigger` past `now` so a slow caller never
    /// double-fires; the caller records outcomes via [`Scheduler::record_fire`]
    /// (which also bumps `run_count` exactly once per fire).
    pub fn tick_once(&mut self, now: DateTime<Utc>) -> Vec<RuntimeSchedule> {
        let mut due = Vec::new();
        for schedule in self.schedules.iter_mut() {
            if !schedule.active || schedule.next_trigger > now {
                continue;
            }
            schedule.last_run = Some(now);
            schedule.next_trigger = schedule.trigger.next_after(now, schedule.last_run);
            due.push(schedule.clone());
        }
        due
    }

    /// Record one fire's outcome and bump `run_count` (also refreshes the
    /// next trigger in case the handler took significant time).
    pub fn record_fire(&mut self, id: &str, status: &str, now: DateTime<Utc>) {
        if let Some(schedule) = self.schedules.iter_mut().find(|s| s.id == id) {
            schedule.last_status = Some(status.to_owned());
            schedule.run_count += 1;
            if schedule.next_trigger <= now {
                schedule.next_trigger = schedule.trigger.next_after(now, schedule.last_run);
            }
        }
    }

    /// Find one schedule by id (used by `trigger_schedule_now`).
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&RuntimeSchedule> {
        self.schedules.iter().find(|s| s.id == id)
    }
}

impl From<&RuntimeSchedule> for ScheduleStatusView {
    fn from(schedule: &RuntimeSchedule) -> Self {
        Self {
            id: schedule.id.clone(),
            name: schedule.name.clone(),
            expression: schedule.expression.clone(),
            cron: schedule.cron.clone(),
            active: schedule.active,
            next_trigger_at: schedule.next_trigger.to_rfc3339(),
            last_run_at: schedule.last_run.map(|instant| instant.to_rfc3339()),
            last_status: schedule.last_status.clone(),
            run_count: schedule.run_count,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, 0)
            .single()
            .expect("valid")
    }

    fn two_schedules(now: DateTime<Utc>) -> Scheduler {
        let tasks = instruction_parser::parse_heartbeat(
            "## fast\n- every 30 minutes\n- ping\n## morning\n- daily at 09:00\n- standup\n",
            None,
        )
        .expect("parses");
        Scheduler::from_tasks(&tasks, now)
    }

    #[test]
    fn loads_two_schedules_with_ids_and_cron() {
        let now = utc(2026, 11, 6, 8, 0);
        let scheduler = two_schedules(now);
        assert_eq!(scheduler.len(), 2);
        let status = scheduler.status();
        assert_eq!(status[0].cron, "*/30 * * * *");
        assert!(status.iter().any(|s| s.id.starts_with("sched-1-morning")));
        assert!(status.iter().all(|s| s.active));
    }

    #[test]
    fn paused_schedules_never_fire() {
        let now = utc(2026, 11, 6, 8, 0);
        let mut scheduler = two_schedules(now);
        for view in scheduler.status() {
            scheduler.set_active(&view.id, false, now);
        }
        // Jump far past every trigger: nothing due while all are paused.
        let due = scheduler.tick_once(utc(2027, 1, 1, 0, 0));
        assert!(due.is_empty(), "paused schedules must not fire");
        // Reactivating one schedule lets exactly it fire.
        let id = scheduler.status()[0].id.clone();
        scheduler.set_active(&id, true, utc(2026, 11, 6, 8, 0));
        let due = scheduler.tick_once(utc(2027, 1, 1, 0, 0));
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].id, id);
    }

    #[test]
    fn tick_fires_due_and_advances_without_double_fire() {
        let now = utc(2026, 11, 6, 8, 0);
        let mut scheduler = two_schedules(now);
        // 08:00 + 30min interval => first due at 08:30; daily 09:00 not yet.
        let due = scheduler.tick_once(utc(2026, 11, 6, 8, 30));
        assert_eq!(due.len(), 1);
        assert!(due[0].id.starts_with("sched-0-fast"));
        scheduler.record_fire(&due[0].id, "completed", utc(2026, 11, 6, 8, 30));
        // Same instant again: nothing (next trigger advanced to 09:00).
        let again = scheduler.tick_once(utc(2026, 11, 6, 8, 30));
        assert!(again.is_empty());
    }

    #[test]
    fn far_future_jump_fires_each_schedule_exactly_once() {
        // Missed-wake bound: jumping a year ahead fires each schedule ONCE,
        // never once per missed interval (no catch-up thundering herd).
        let now = utc(2026, 11, 6, 8, 0);
        let mut scheduler = two_schedules(now);
        let due = scheduler.tick_once(utc(2027, 11, 6, 8, 0));
        assert_eq!(due.len(), 2, "one fire per schedule, not per missed tick");
        for schedule in &due {
            scheduler.record_fire(&schedule.id, "completed", utc(2027, 11, 6, 8, 0));
        }
        let again = scheduler.tick_once(utc(2027, 11, 6, 8, 0));
        assert!(again.is_empty(), "no immediate refire after recording");
    }

    #[test]
    fn rebuild_after_fire_does_not_refire_the_same_interval() {
        // Restart simulation: the daemon rebuilds scheduler state from
        // HEARTBEAT.md (last_run unknown), anchoring forward from boot time.
        // An interval that already fired must not fire again.
        let mut scheduler = two_schedules(utc(2026, 11, 6, 8, 0));
        let due = scheduler.tick_once(utc(2026, 11, 6, 8, 30));
        assert_eq!(due.len(), 1);
        assert!(due[0].id.starts_with("sched-0-fast"));
        // "Restart" five minutes later: rebuilt state fires nothing —
        // the 08:30 interval was already consumed.
        let mut rebuilt = two_schedules(utc(2026, 11, 6, 8, 35));
        let due = rebuilt.tick_once(utc(2026, 11, 6, 8, 35));
        assert!(
            due.is_empty(),
            "restart must not refire a consumed interval"
        );
    }

    #[test]
    fn clock_skew_backward_never_refires() {
        let mut scheduler = two_schedules(utc(2026, 11, 6, 8, 0));
        let due = scheduler.tick_once(utc(2026, 11, 6, 8, 30));
        assert_eq!(due.len(), 1);
        // Clock jumps back five minutes: next trigger (09:00) is still in
        // the future, so nothing fires.
        let skewed = scheduler.tick_once(utc(2026, 11, 6, 8, 25));
        assert!(skewed.is_empty(), "backward skew never refires");
    }

    #[test]
    fn set_active_unknown_id_returns_none() {
        let mut scheduler = two_schedules(utc(2026, 11, 6, 8, 0));
        assert!(scheduler
            .set_active("sched-9-nope", false, utc(2026, 11, 6, 8, 0))
            .is_none());
    }

    #[test]
    fn missing_file_loads_empty() {
        let scheduler = Scheduler::load_file(
            std::path::Path::new("/nonexistent/HEARTBEAT.md"),
            utc(2026, 11, 6, 8, 0),
        );
        assert!(scheduler.is_empty());
    }

    #[test]
    fn run_count_and_last_status_track_fires() {
        let now = utc(2026, 11, 6, 8, 0);
        let mut scheduler = two_schedules(now);
        let due = scheduler.tick_once(utc(2026, 11, 6, 8, 30));
        scheduler.record_fire(&due[0].id, "waiting_policy", utc(2026, 11, 6, 8, 30));
        let view = scheduler
            .status()
            .into_iter()
            .find(|s| s.id == due[0].id)
            .expect("view");
        assert_eq!(view.run_count, 1);
        assert_eq!(view.last_status.as_deref(), Some("waiting_policy"));
    }
}
