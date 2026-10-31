//! UTC timestamp helpers. Talos persists RFC 3339 / ISO-8601 strings.

use chrono::{DateTime, Utc};

use crate::error::{Result, TalosError};

/// Current UTC time.
#[must_use]
pub fn now_utc() -> DateTime<Utc> {
    Utc::now()
}

/// Current UTC time as an RFC 3339 string.
#[must_use]
pub fn now_iso() -> String {
    now_utc().to_rfc3339()
}

/// Parse an RFC 3339 timestamp produced by [`now_iso`].
pub fn parse_iso(raw: &str) -> Result<DateTime<Utc>> {
    raw.parse::<DateTime<Utc>>()
        .map_err(|err| TalosError::Time(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_round_trip() {
        let raw = now_iso();
        let parsed = parse_iso(&raw).expect("produced timestamps parse");
        assert_eq!(parsed.to_rfc3339(), raw);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_iso("not-a-time").is_err());
    }
}
