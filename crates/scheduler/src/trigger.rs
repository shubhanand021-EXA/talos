//! Deterministic schedule triggers: the time half of HEARTBEAT.md.
//!
//! Every [`Trigger`] is total over UTC wall-clock time: [`Trigger::next_after`]
//! always returns the strictly-next firing instant for a fixed `now`, so tests
//! and the daemon agree exactly. [`Trigger::to_cron`] renders the equivalent
//! five-field cron expression for display and export.

use chrono::{DateTime, Datelike, TimeZone, Utc, Weekday};

/// A parsed schedule ready to fire. Mirrors
/// `instruction_parser::{ScheduleKind, ScheduledTask}` without depending on
/// its exact shape (the engine only needs time fields + an id).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Trigger {
    /// Fire every N minutes from the last run (or `now` when never run).
    Interval {
        /// Period in minutes (1..=1_000_000 as parsed).
        every_minutes: u64,
    },
    /// Fire daily at a wall-clock time (UTC).
    Daily {
        /// Hour 0..=23.
        hour: u32,
        /// Minute 0..=59.
        minute: u32,
    },
    /// Fire weekly on a weekday at a wall-clock time (UTC).
    Weekly {
        /// Weekday the schedule fires on.
        weekday: Weekday,
        /// Hour 0..=23.
        hour: u32,
        /// Minute 0..=59.
        minute: u32,
    },
    /// Fire monthly on a day-of-month at a wall-clock time (UTC). Months
    /// shorter than `day` clamp to their last day.
    Monthly {
        /// Day of month 1..=31.
        day: u32,
        /// Hour 0..=23.
        hour: u32,
        /// Minute 0..=59.
        minute: u32,
    },
}

/// Build a [`Trigger`] from one parsed HEARTBEAT.md task. Returns `None`
/// when the task carries no usable time fields (never panics on bad input).
#[must_use]
pub fn trigger_for(task: &instruction_parser::ScheduledTask) -> Option<Trigger> {
    match task.kind {
        instruction_parser::ScheduleKind::Interval => {
            let minutes = task.every_minutes?;
            if minutes == 0 {
                return None;
            }
            Some(Trigger::Interval {
                every_minutes: minutes,
            })
        }
        instruction_parser::ScheduleKind::Daily => {
            let (hour, minute) = task.at.as_deref().and_then(parse_hhmm)?;
            Some(Trigger::Daily { hour, minute })
        }
        instruction_parser::ScheduleKind::Weekly => {
            let (hour, minute) = task.at.as_deref().and_then(parse_hhmm)?;
            let weekday = task.weekday.as_deref().and_then(parse_weekday)?;
            Some(Trigger::Weekly {
                weekday,
                hour,
                minute,
            })
        }
        instruction_parser::ScheduleKind::Monthly => {
            let (hour, minute) = task.at.as_deref().and_then(parse_hhmm)?;
            let day = task.day_of_month?;
            if !(1..=31).contains(&day) {
                return None;
            }
            Some(Trigger::Monthly { day, hour, minute })
        }
    }
}

/// Parse a normalized `HH:MM` 24h time into `(hour, minute)`.
#[must_use]
pub fn parse_hhmm(value: &str) -> Option<(u32, u32)> {
    let (hour, minute) = value.split_once(':')?;
    match (hour.parse::<u32>(), minute.parse::<u32>()) {
        (Ok(hour), Ok(minute)) => {
            if hour < 24 && minute < 60 && value.len() == 5 {
                Some((hour, minute))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Parse a lowercase weekday name.
fn parse_weekday(value: &str) -> Option<Weekday> {
    match value {
        "monday" => Some(Weekday::Mon),
        "tuesday" => Some(Weekday::Tue),
        "wednesday" => Some(Weekday::Wed),
        "thursday" => Some(Weekday::Thu),
        "friday" => Some(Weekday::Fri),
        "saturday" => Some(Weekday::Sat),
        "sunday" => Some(Weekday::Sun),
        _ => None,
    }
}

impl Trigger {
    /// Strictly-next firing instant after `now` (UTC). `last_run` anchors
    /// interval schedules; wall-clock schedules ignore it.
    #[must_use]
    pub fn next_after(&self, now: DateTime<Utc>, last_run: Option<DateTime<Utc>>) -> DateTime<Utc> {
        match self {
            Trigger::Interval { every_minutes } => {
                let period = chrono::Duration::minutes(*every_minutes as i64);
                let anchor = last_run.unwrap_or(now);
                let mut next = anchor + period;
                // If the daemon slept past several periods, skip ahead instead
                // of firing a burst (at most one catch-up step walks forward).
                let mut guard = 0;
                while next <= now && guard < 1_000_000 {
                    next += period;
                    guard += 1;
                }
                next
            }
            Trigger::Daily { hour, minute } => {
                let candidate = midnight(now)
                    + chrono::Duration::hours(*hour as i64)
                    + chrono::Duration::minutes(*minute as i64);
                if candidate > now {
                    candidate
                } else {
                    candidate + chrono::Duration::days(1)
                }
            }
            Trigger::Weekly {
                weekday,
                hour,
                minute,
            } => {
                let today = midnight(now)
                    + chrono::Duration::hours(*hour as i64)
                    + chrono::Duration::minutes(*minute as i64);
                let days_ahead = (weekday.num_days_from_monday() as i64
                    - now.weekday().num_days_from_monday() as i64)
                    .rem_euclid(7);
                let candidate = today + chrono::Duration::days(days_ahead);
                if candidate > now {
                    candidate
                } else {
                    candidate + chrono::Duration::days(7)
                }
            }
            Trigger::Monthly { day, hour, minute } => next_monthly(now, *day, *hour, *minute),
        }
    }

    /// Equivalent five-field cron expression (UTC) for display and export.
    #[must_use]
    pub fn to_cron(&self) -> String {
        match self {
            Trigger::Interval { every_minutes } => {
                if *every_minutes < 60 {
                    format!("*/{every_minutes} * * * *")
                } else if every_minutes % 60 == 0 {
                    format!("0 */{} * * *", every_minutes / 60)
                } else {
                    format!("0 {} * * *", every_minutes / 60)
                }
            }
            Trigger::Daily { hour, minute } => format!("{minute} {hour} * * *"),
            Trigger::Weekly {
                weekday,
                hour,
                minute,
            } => {
                // Cron day-of-week: Sunday = 0 (chrono num_days_from_sunday matches).
                format!("{minute} {hour} * * {}", weekday.num_days_from_sunday())
            }
            Trigger::Monthly { day, hour, minute } => format!("{minute} {hour} {day} * *"),
        }
    }
}

/// Next monthly firing strictly after `now`, clamping `day` to short months.
fn next_monthly(now: DateTime<Utc>, day: u32, hour: u32, minute: u32) -> DateTime<Utc> {
    let (year, month) = (now.year(), now.month());
    if let Some(candidate) = monthly_candidate(year, month, day, hour, minute) {
        if candidate > now {
            return candidate;
        }
    }
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    monthly_candidate(next_year, next_month, day, hour, minute).expect("clamped day always exists")
}

/// One month's firing instant, clamping `day` to the month's length.
fn monthly_candidate(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
) -> Option<DateTime<Utc>> {
    let last = last_day_of_month(year, month);
    let clamped = day.min(last);
    Utc.with_ymd_and_hms(year, month, clamped, hour, minute, 0)
        .single()
}

/// Last calendar day of a month (leap-year aware, proleptic Gregorian).
fn last_day_of_month(year: i32, month: u32) -> u32 {
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    let first_of_next =
        chrono::NaiveDate::from_ymd_opt(next_year, next_month, 1).expect("valid month address");
    first_of_next.pred_opt().expect("month has days").day()
}

/// UTC midnight starting `instant`'s date.
fn midnight(instant: DateTime<Utc>) -> DateTime<Utc> {
    instant
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .map(|naive| naive.and_utc())
        .expect("midnight exists")
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

    #[test]
    fn interval_advances_from_last_run_and_skips_bursts() {
        let trigger = Trigger::Interval { every_minutes: 30 };
        let now = utc(2026, 11, 6, 12, 0);
        // Fresh schedule fires one period after now.
        assert_eq!(trigger.next_after(now, None), utc(2026, 11, 6, 12, 30));
        // Anchored on the last run.
        assert_eq!(
            trigger.next_after(now, Some(utc(2026, 11, 6, 11, 50))),
            utc(2026, 11, 6, 12, 20)
        );
        // Slept daemon: skip whole missed periods, never a burst.
        assert_eq!(
            trigger.next_after(now, Some(utc(2026, 11, 6, 9, 0))),
            utc(2026, 11, 6, 12, 30)
        );
    }

    #[test]
    fn daily_fires_next_occurrence_strictly_after_now() {
        let trigger = Trigger::Daily { hour: 9, minute: 0 };
        assert_eq!(
            trigger.next_after(utc(2026, 11, 6, 8, 59), None),
            utc(2026, 11, 6, 9, 0)
        );
        // Exact hit rolls to tomorrow (strictly after).
        assert_eq!(
            trigger.next_after(utc(2026, 11, 6, 9, 0), None),
            utc(2026, 11, 7, 9, 0)
        );
        assert_eq!(
            trigger.next_after(utc(2026, 11, 6, 9, 1), None),
            utc(2026, 11, 7, 9, 0)
        );
    }

    #[test]
    fn weekly_fires_on_the_right_weekday() {
        // 2026-11-06 is a Friday.
        let trigger = Trigger::Weekly {
            weekday: Weekday::Fri,
            hour: 17,
            minute: 30,
        };
        assert_eq!(
            trigger.next_after(utc(2026, 11, 6, 12, 0), None),
            utc(2026, 11, 6, 17, 30)
        );
        assert_eq!(
            trigger.next_after(utc(2026, 11, 6, 18, 0), None),
            utc(2026, 11, 13, 17, 30)
        );
        let monday = Trigger::Weekly {
            weekday: Weekday::Mon,
            hour: 9,
            minute: 0,
        };
        assert_eq!(
            monday.next_after(utc(2026, 11, 6, 18, 0), None),
            utc(2026, 11, 9, 9, 0)
        );
    }

    #[test]
    fn monthly_clamps_short_months_and_includes_leap_day() {
        let trigger = Trigger::Monthly {
            day: 31,
            hour: 10,
            minute: 0,
        };
        // February 2027 has 28 days: clamp to the 28th.
        assert_eq!(
            trigger.next_after(utc(2027, 2, 1, 0, 0), None),
            utc(2027, 2, 28, 10, 0)
        );
        // After the clamped firing, roll to March 31st.
        assert_eq!(
            trigger.next_after(utc(2027, 2, 28, 10, 0), None),
            utc(2027, 3, 31, 10, 0)
        );
        let first = Trigger::Monthly {
            day: 1,
            hour: 9,
            minute: 0,
        };
        assert_eq!(
            first.next_after(utc(2026, 11, 6, 0, 0), None),
            utc(2026, 12, 1, 9, 0)
        );
        assert_eq!(
            first.next_after(utc(2026, 11, 1, 8, 59), None),
            utc(2026, 11, 1, 9, 0)
        );
        // Leap day exists in 2028: from Jan 30 the next 29th is Feb 29.
        let leap = Trigger::Monthly {
            day: 29,
            hour: 0,
            minute: 0,
        };
        assert_eq!(
            leap.next_after(utc(2028, 1, 15, 0, 0), None),
            utc(2028, 1, 29, 0, 0)
        );
        assert_eq!(
            leap.next_after(utc(2028, 1, 30, 0, 0), None),
            utc(2028, 2, 29, 0, 0)
        );
        // Non-leap February clamps the 29th to the 28th.
        assert_eq!(
            leap.next_after(utc(2027, 1, 30, 0, 0), None),
            utc(2027, 2, 28, 0, 0)
        );
    }

    #[test]
    fn cron_strings_match_five_field_shape() {
        assert_eq!(
            Trigger::Interval { every_minutes: 30 }.to_cron(),
            "*/30 * * * *"
        );
        assert_eq!(
            Trigger::Interval { every_minutes: 120 }.to_cron(),
            "0 */2 * * *"
        );
        assert_eq!(Trigger::Daily { hour: 9, minute: 0 }.to_cron(), "0 9 * * *");
        assert_eq!(
            Trigger::Weekly {
                weekday: Weekday::Fri,
                hour: 17,
                minute: 30
            }
            .to_cron(),
            "30 17 * * 5"
        );
        assert_eq!(
            Trigger::Monthly {
                day: 1,
                hour: 9,
                minute: 0
            }
            .to_cron(),
            "0 9 1 * *"
        );
    }

    #[test]
    fn triggers_derive_from_parsed_heartbeat_tasks() {
        let tasks = instruction_parser::parse_heartbeat(
            "## standup\n- every morning at 9am\n- sync\n## bills\n- monthly on the 15th at 10:00\n- pay\n",
            None,
        )
        .expect("parses");
        let standup = trigger_for(&tasks[0]).expect("trigger");
        assert_eq!(standup, Trigger::Daily { hour: 9, minute: 0 });
        let bills = trigger_for(&tasks[1]).expect("trigger");
        assert_eq!(
            bills,
            Trigger::Monthly {
                day: 15,
                hour: 10,
                minute: 0
            }
        );
    }

    #[test]
    fn parse_hhmm_rejects_garbage() {
        assert_eq!(parse_hhmm("09:00"), Some((9, 0)));
        assert_eq!(parse_hhmm("9:00"), None);
        assert_eq!(parse_hhmm("24:00"), None);
        assert_eq!(parse_hhmm("09:60"), None);
        assert_eq!(parse_hhmm("09-00"), None);
    }
}
