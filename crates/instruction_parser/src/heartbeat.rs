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
    /// Wall-clock time `HH:MM` (daily/weekly).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
    /// Weekday name (weekly only, lowercase).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weekday: Option<String>,
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
/// - `every <N> minutes|hours` → interval (`hours` multiply by 60).
/// - `daily at HH:MM` → daily.
/// - `weekly on <weekday> at HH:MM` → weekly.
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
        let (kind, every_minutes, at, weekday, payload_lines) = parse_section(&lines)?;
        tasks.push(ScheduledTask {
            name,
            expression: lines.join("\n"),
            kind,
            every_minutes,
            at,
            weekday,
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
        Vec<String>,
    ),
    crate::ParseError,
> {
    let mut kind: Option<(ScheduleKind, Option<u64>, Option<String>, Option<String>)> = None;
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
            kind = Some((ScheduleKind::Interval, Some(schedule), None, None));
        } else if let Some(at) = parse_daily(&body_lower) {
            if kind.is_some() {
                return Err(crate::ParseError::new(
                    "heartbeat",
                    "only one schedule expression per task",
                ));
            }
            kind = Some((ScheduleKind::Daily, None, Some(at), None));
        } else if let Some((weekday, at)) = parse_weekly(&body_lower) {
            if kind.is_some() {
                return Err(crate::ParseError::new(
                    "heartbeat",
                    "only one schedule expression per task",
                ));
            }
            kind = Some((ScheduleKind::Weekly, None, Some(at), Some(weekday)));
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
    let Some((schedule_kind, every_minutes, at, weekday)) = kind else {
        return Err(crate::ParseError::new(
            "heartbeat",
            "each task needs one schedule expression (every N minutes|hours, daily at HH:MM, weekly on <day> at HH:MM)",
        ));
    };
    Ok((schedule_kind, every_minutes, at, weekday, payload))
}

fn parse_every(lower: &str) -> Option<u64> {
    let rest = lower.strip_prefix("every ")?.trim();
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
    let rest = lower.strip_prefix("daily at ")?.trim();
    valid_hhmm(rest).then(|| rest.to_owned())
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
    valid_hhmm(at).then(|| (weekday, at.to_owned()))
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
