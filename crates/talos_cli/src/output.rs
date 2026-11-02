//! Output contract: human text vs JSONL for CI/CD parsing.
//!
//! `--format json` prints exactly one JSON object per line on stdout (JSONL).
//! Each object carries `type` (the event name), `timestamp` (RFC 3339), and
//! the event payload. Parsers must ignore unknown fields (forward
//! compatibility) and must treat stderr as diagnostics, never events.

use talos_common::now_iso;

/// Output shape selected by `--format`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Human-readable lines.
    Text,
    /// One JSON object per line.
    Json,
}

/// Build one JSONL event object (adds `type` + `timestamp`).
#[must_use]
pub fn jsonl_event(event_type: &str, payload: serde_json::Value) -> String {
    let mut obj = match payload {
        serde_json::Value::Object(map) => map,
        other => {
            let mut map = serde_json::Map::new();
            map.insert("data".to_owned(), other);
            map
        }
    };
    obj.insert(
        "type".to_owned(),
        serde_json::Value::String(event_type.to_owned()),
    );
    obj.insert("timestamp".to_owned(), serde_json::Value::String(now_iso()));
    serde_json::Value::Object(obj).to_string()
}

/// Print one JSONL event to stdout.
pub fn emit_jsonl(event_type: &str, payload: serde_json::Value) {
    println!("{}", jsonl_event(event_type, payload));
}

/// Print a human-readable line to stdout.
pub fn print_text(line: &str) {
    println!("{line}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsonl_event_has_type_and_timestamp() {
        let line = jsonl_event("task_started", serde_json::json!({"task_id": "t-1"}));
        let value: serde_json::Value = serde_json::from_str(&line).expect("parses");
        assert_eq!(value["type"], "task_started");
        assert_eq!(value["task_id"], "t-1");
        assert!(value["timestamp"].as_str().is_some_and(|s| !s.is_empty()));
        // Single line: JSONL framing.
        assert!(!line.contains('\n'));
    }

    #[test]
    fn jsonl_event_wraps_non_objects() {
        let line = jsonl_event("cost_update", serde_json::json!(1.5));
        let value: serde_json::Value = serde_json::from_str(&line).expect("parses");
        assert_eq!(value["data"], 1.5);
    }
}
