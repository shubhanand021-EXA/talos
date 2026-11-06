//! HEARTBEAT.md parser: natural-language schedule expressions →
//! `ScheduledTask` definitions. Execution arrives in Batch 5; this module only
//! parses and stores deterministic definitions (no timers, no spawning).

/// Supported schedule shapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScheduleKind {
    /// Run every N minutes.
    Interval,
    /// Run at a daily wall-clock time.
    Daily,
    /// Run weekly on a weekday at a wall-clock time.
    Weekly,
    /// Run monthly on a day-of-month at a wall-clock time.
    Monthly,
}

/// One parsed scheduled task (execution in Batch 5).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledTask {
    /// Task name (heading or first line).
    pub name: String,
    /// Original natural-language expression (verbatim).
    pub expression: String,
    /// Schedule shape.
    pub kind: ScheduleKind,
    /// Interval minutes (interval only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub every_minutes: Option<u64>,
    /// Wall-clock time `HH:MM` (daily/weekly/monthly, normalized 24h).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
    /// Weekday name (weekly only, lowercase).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weekday: Option<String>,
    /// Day of month 1..=31 (monthly only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub day_of_month: Option<u32>,
    /// What to run (remaining body, verbatim).
    pub payload: String,
    /// Source path the task was read from, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
}

/// Parse HEARTBEAT.md text into scheduled tasks.
///
/// Grammar (deterministic, line-oriented):
/// - `## <name>` headings start a task; lines before the first heading belong
///   to an implicit `heartbeat` task.
/// - `every <N> minutes|hours`, `every hour` / `hourly` → interval.
/// - `every morning|evening|night|noon|midnight [at <time>]` → daily
///   (09:00 / 18:00 / 23:00 / 12:00 / 00:00 defaults).
/// - `every day [at <time>]` → daily (09:00 default).
/// - `daily at <time>` → daily.
/// - `weekly on <weekday> at <time>` → weekly.
/// - `monthly on the <N>th [at <time>]`, `on the <N>st|nd|rd|th of the month
///   [at <time>]`, `every month [at <time>]` → monthly (day 1, 09:00 defaults).
/// - `<time>` is `HH:MM` 24h or `9am` / `9:30pm` 12h.
/// - Remaining non-schedule lines are the payload.
pub fn parse_heartbeat(
    text: &str,
    source_path: Option<String>,
) -> Result<Vec<ScheduledTask>, crate::ParseError> {
    if text.len() > 262_144 {
        return Err(crate::ParseError::new(
            "heartbeat",
            "HEARTBEAT.md must be at most 256 KiB",
        ));
    }
    // Split into (name, lines) sections.
    let mut sections: Vec<(String, Vec<String>)> = vec![("heartbeat".to_owned(), Vec::new())];
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(name) = line.strip_prefix("##").map(str::trim) {
            if !name.is_empty() {
                sections.push((bounded_name(name)?, Vec::new()));
                continue;
            }
        }
        if line.starts_with('#') {
            continue;
        }
        sections
            .last_mut()
            .expect("at least one section")
            .1
            .push(line.to_owned());
    }
    let mut tasks = Vec::new();
    for (name, lines) in sections {
        if lines.is_empty() {
            continue;
        }
        let (kind, every_minutes, at, weekday, day_of_month, payload_lines) =
            parse_section(&lines)?;
        tasks.push(ScheduledTask {
            name,
            expression: lines.join("\n"),
            kind,
            every_minutes,
            at,
            weekday,
            day_of_month,
            payload: payload_lines.join("\n"),
            source_path: source_path.clone(),
        });
    }
    if tasks.is_empty() {
        return Err(crate::ParseError::new(
            "heartbeat",
            "no scheduled tasks found",
        ));
    }
    if tasks.len() > 32 {
        return Err(crate::ParseError::new(
            "heartbeat",
            "at most 32 tasks per file",
        ));
    }
    Ok(tasks)
}

fn bounded_name(name: &str) -> Result<String, crate::ParseError> {
    if name.is_empty() || name.len() > 128 {
        return Err(crate::ParseError::new(
            "heartbeat",
            "task names must be 1..=128 chars",
        ));
    }
    Ok(name.to_owned())
}

#[allow(clippy::type_complexity)]
fn parse_section(
    lines: &[String],
) -> Result<
    (
        ScheduleKind,
        Option<u64>,
        Option<String>,
        Option<String>,
        Option<u32>,
        Vec<String>,
    ),
    crate::ParseError,
> {
    let mut kind: Option<(
        ScheduleKind,
        Option<u64>,
        Option<String>,
        Option<String>,
        Option<u32>,
    )> = None;
    let mut payload = Vec::new();
    for line in lines {
        let lower = line.to_ascii_lowercase();
        let body = line.strip_prefix('-').map(str::trim).unwrap_or(line);
        let body_lower = body.to_ascii_lowercase();
        if let Some(schedule) = parse_every(&body_lower) {
            if kind.is_some() {
                return Err(crate::ParseError::new(
                    "heartbeat",
                    "only one schedule expression per task",
                ));
            }
            kind = Some((ScheduleKind::Interval, Some(schedule), None, None, None));
        } else if let Some((at, day_of_month)) = parse_monthly(&body_lower) {
            if kind.is_some() {
                return Err(crate::ParseError::new(
                    "heartbeat",
                    "only one schedule expression per task",
                ));
            }
            kind = Some((
                ScheduleKind::Monthly,
                None,
                Some(at),
                None,
                Some(day_of_month),
            ));
        } else if let Some(at) = parse_daily(&body_lower) {
            if kind.is_some() {
                return Err(crate::ParseError::new(
                    "heartbeat",
                    "only one schedule expression per task",
                ));
            }
            kind = Some((ScheduleKind::Daily, None, Some(at), None, None));
        } else if let Some((weekday, at)) = parse_weekly(&body_lower) {
            if kind.is_some() {
                return Err(crate::ParseError::new(
                    "heartbeat",
                    "only one schedule expression per task",
                ));
            }
            kind = Some((ScheduleKind::Weekly, None, Some(at), Some(weekday), None));
        } else {
            if body.len() > 4096 {
                return Err(crate::ParseError::new(
                    "heartbeat",
                    "payload lines must be at most 4096 chars",
                ));
            }
            let _ = lower;
            payload.push(body.to_owned());
        }
    }
    let Some((schedule_kind, every_minutes, at, weekday, day_of_month)) = kind else {
        return Err(crate::ParseError::new(
            "heartbeat",
            "each task needs one schedule expression (every N minutes|hours, every morning|evening|night, daily at <time>, weekly on <day> at <time>, monthly on the <N>th [at <time>])",
        ));
    };
    Ok((
        schedule_kind,
        every_minutes,
        at,
        weekday,
        day_of_month,
        payload,
    ))
}

fn parse_every(lower: &str) -> Option<u64> {
    if lower == "hourly" {
        return Some(60);
    }
    let rest = lower.strip_prefix("every ")?.trim();
    if rest == "hour" || rest == "an hour" {
        return Some(60);
    }
    // `every morning|evening|night|noon|midnight|day` are daily shorthands,
    // handled by parse_daily so the Interval branch stays minute-exact.
    let (number, unit) = rest.split_once(' ')?;
    let amount: u64 = number.trim().parse().ok()?;
    if amount == 0 || amount > 1_000_000 {
        return None;
    }
    match unit.trim() {
        "minute" | "minutes" => Some(amount),
        "hour" | "hours" => Some(amount.saturating_mul(60)),
        _ => None,
    }
}

fn parse_daily(lower: &str) -> Option<String> {
    // Named dayparts with an optional explicit override:
    // `every morning at 9am`, `every evening`, `every day at 17:00`.
    for (word, default) in [
        ("morning", "09:00"),
        ("evening", "18:00"),
        ("night", "23:00"),
        ("noon", "12:00"),
        ("midday", "12:00"),
        ("midnight", "00:00"),
        ("day", "09:00"),
    ] {
        if let Some(tail) = lower.strip_prefix(&format!("every {word}")) {
            let tail = tail.trim();
            if tail.is_empty() {
                return Some(default.to_owned());
            }
            if let Some(at) = tail.strip_prefix("at ").map(str::trim) {
                return parse_time(at);
            }
            return None;
        }
    }
    let rest = lower.strip_prefix("daily at ")?.trim();
    parse_time(rest)
}

fn parse_weekly(lower: &str) -> Option<(String, String)> {
    let rest = lower.strip_prefix("weekly on ")?.trim();
    let (weekday, at_part) = rest.split_once(" at ")?;
    let weekday = weekday.trim().to_owned();
    if ![
        "monday",
        "tuesday",
        "wednesday",
        "thursday",
        "friday",
        "saturday",
        "sunday",
    ]
    .contains(&weekday.as_str())
    {
        return None;
    }
    let at = at_part.trim();
    parse_time(at).map(|normalized| (weekday, normalized))
}

/// Monthly schedules: `monthly on the 15th [at 10:00]`,
/// `on the 1st of the month [at 9am]`, `every month [at 09:00]`.
/// Defaults: day 1, 09:00.
fn parse_monthly(lower: &str) -> Option<(String, u32)> {
    const DEFAULT_AT: &str = "09:00";
    if let Some(tail) = lower.strip_prefix("monthly on the ") {
        let (day, at) = split_day_and_at(tail)?;
        return Some((at.unwrap_or_else(|| DEFAULT_AT.to_owned()), day));
    }
    if let Some(tail) = lower.strip_prefix("on the ") {
        let tail = tail
            .strip_suffix("of the month")
            .map(str::trim)
            .unwrap_or(tail);
        let (day, at) = split_day_and_at(tail)?;
        return Some((at.unwrap_or_else(|| DEFAULT_AT.to_owned()), day));
    }
    if let Some(tail) = lower.strip_prefix("every month") {
        let tail = tail.trim();
        if tail.is_empty() {
            return Some((DEFAULT_AT.to_owned(), 1));
        }
        if let Some(at) = tail.strip_prefix("at ").map(str::trim) {
            return parse_time(at).map(|normalized| (normalized, 1));
        }
        return None;
    }
    None
}

/// Split `15th [at <time>]` into `(day, Option<normalized time>)`.
fn split_day_and_at(tail: &str) -> Option<(u32, Option<String>)> {
    let tail = tail.trim();
    let (day_part, at_part) = match tail.split_once(" at ") {
        Some((day, at)) => (day.trim(), Some(at.trim())),
        None => (tail, None),
    };
    let day = parse_ordinal(day_part)?;
    let at = match at_part {
        Some(raw) => Some(parse_time(raw)?),
        None => None,
    };
    Some((day, at))
}

/// Parse `1st` / `2nd` / `3rd` / `15th` into a day-of-month 1..=31.
fn parse_ordinal(value: &str) -> Option<u32> {
    let digits = value
        .strip_suffix("st")
        .or_else(|| value.strip_suffix("nd"))
        .or_else(|| value.strip_suffix("rd"))
        .or_else(|| value.strip_suffix("th"))
        .unwrap_or(value);
    let day: u32 = digits.trim().parse().ok()?;
    (1..=31).contains(&day).then_some(day)
}

/// Parse a wall-clock time: `HH:MM` 24h or `9am` / `9:30pm` 12h.
/// Returns the normalized `HH:MM` 24h form.
fn parse_time(value: &str) -> Option<String> {
    let value = value.trim();
    if valid_hhmm(value) {
        return Some(value.to_owned());
    }
    let lower = value.to_ascii_lowercase();
    let (digits, meridiem) = if let Some(stem) = lower.strip_suffix("am") {
        (stem, 0u32)
    } else {
        (lower.strip_suffix("pm")?, 12u32)
    };
    let (hour_raw, minute_raw) = match digits.split_once(':') {
        Some((hour, minute)) => (hour.trim(), minute.trim()),
        None => (digits.trim(), "00"),
    };
    let hour12_raw: u32 = hour_raw.parse().ok()?;
    let minute: u32 = minute_raw.parse().ok()?;
    if !(1..=12).contains(&hour12_raw) || minute > 59 {
        return None;
    }
    let hour24 = (hour12_raw % 12) + meridiem;
    Some(format!("{hour24:02}:{minute:02}"))
}

fn valid_hhmm(value: &str) -> bool {
    let Some((hour, minute)) = value.split_once(':') else {
        return false;
    };
    match (hour.parse::<u32>(), minute.parse::<u32>()) {
        (Ok(hour), Ok(minute)) => hour < 24 && minute < 60 && value.len() == 5,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_interval_hours_and_minutes() {
        let tasks =
            parse_heartbeat("## sync\n- every 2 hours\n- summarize the ledger\n", None).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].kind, ScheduleKind::Interval);
        assert_eq!(tasks[0].every_minutes, Some(120));
        assert_eq!(tasks[0].payload, "summarize the ledger");
    }

    #[test]
    fn parses_daily_and_weekly() {
        let tasks = parse_heartbeat(
            "## morning\n- daily at 09:00\n- open standup\n## review\n- weekly on friday at 17:30\n- ship notes\n",
            None,
        )
        .unwrap();
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].kind, ScheduleKind::Daily);
        assert_eq!(tasks[0].at.as_deref(), Some("09:00"));
        assert_eq!(tasks[1].kind, ScheduleKind::Weekly);
        assert_eq!(tasks[1].weekday.as_deref(), Some("friday"));
        assert_eq!(tasks[1].at.as_deref(), Some("17:30"));
    }

    #[test]
    fn parses_dayparts_and_twelve_hour_times() {
        let tasks = parse_heartbeat(
            "## standup\n- every morning at 9am\n- sync\n## close\n- every evening\n- wrap\n## noon\n- daily at 12:30pm\n- check\n",
            None,
        )
        .unwrap();
        assert_eq!(tasks.len(), 3);
        assert_eq!(tasks[0].kind, ScheduleKind::Daily);
        assert_eq!(tasks[0].at.as_deref(), Some("09:00"));
        assert_eq!(tasks[1].at.as_deref(), Some("18:00"));
        assert_eq!(tasks[2].at.as_deref(), Some("12:30"));
        let weekly =
            parse_heartbeat("## review\n- weekly on friday at 5pm\n- notes\n", None).unwrap();
        assert_eq!(weekly[0].at.as_deref(), Some("17:00"));
    }

    #[test]
    fn parses_midnight_noon_and_hourly_shorthands() {
        let tasks = parse_heartbeat(
            "## a\n- every midnight\n- x\n## b\n- every noon\n- y\n## c\n- every hour\n- z\n## d\n- hourly\n- w\n",
            None,
        )
        .unwrap();
        assert_eq!(tasks[0].at.as_deref(), Some("00:00"));
        assert_eq!(tasks[1].at.as_deref(), Some("12:00"));
        assert_eq!(tasks[2].kind, ScheduleKind::Interval);
        assert_eq!(tasks[2].every_minutes, Some(60));
        assert_eq!(tasks[3].every_minutes, Some(60));
    }

    #[test]
    fn parses_monthly_schedules() {
        let tasks = parse_heartbeat(
            "## billing\n- monthly on the 15th at 10:00\n- invoice\n## rent\n- on the 1st of the month\n- pay\n## report\n- every month at 9am\n- summarize\n",
            None,
        )
        .unwrap();
        assert_eq!(tasks.len(), 3);
        assert_eq!(tasks[0].kind, ScheduleKind::Monthly);
        assert_eq!(tasks[0].day_of_month, Some(15));
        assert_eq!(tasks[0].at.as_deref(), Some("10:00"));
        assert_eq!(tasks[1].day_of_month, Some(1));
        assert_eq!(tasks[1].at.as_deref(), Some("09:00"));
        assert_eq!(tasks[2].day_of_month, Some(1));
        assert_eq!(tasks[2].at.as_deref(), Some("09:00"));
    }

    #[test]
    fn rejects_missing_schedule() {
        assert!(parse_heartbeat("## vague\n- just do things\n", None).is_err());
        assert!(parse_heartbeat("", None).is_err());
    }

    #[test]
    fn deterministic_parse() {
        let text = "## a\n- every 5 minutes\n- go\n";
        assert_eq!(
            parse_heartbeat(text, None).unwrap(),
            parse_heartbeat(text, None).unwrap()
        );
    }
}
