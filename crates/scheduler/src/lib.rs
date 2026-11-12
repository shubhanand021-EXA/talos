//! HEARTBEAT natural-language scheduler: deterministic triggers, cron
//! mapping, and a tick engine for the Talos gateway.
//!
//! The parser (`instruction_parser::heartbeat`) owns the file grammar. This
//! crate owns *time*: every parsed [`ScheduledTask`] maps to a [`Trigger`]
//! with a total [`Trigger::next_after`] function (UTC, deterministic for a
//! fixed `now`) and a [`Trigger::to_cron`] display string. The [`Scheduler`]
//! engine ticks on a Tokio interval, fires due schedules through a caller
//! handler (the gateway submits system tasks into the Supervisor), and tracks
//! per-schedule active flags, last runs, and outcomes. Paused schedules never
//! fire.

pub mod engine;
pub mod trigger;

pub use engine::{RuntimeSchedule, ScheduleStatusView, Scheduler};
pub use trigger::{parse_hhmm, trigger_for, Trigger};
