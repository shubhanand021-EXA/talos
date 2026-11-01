//! Local JSON file export (one span per line) + retention cleanup.
//!
//! Privacy: export requires explicit user action (IPC `export_telemetry`);
//! nothing is transmitted. Old files are purged by [`cleanup_old_exports`]
//! using the same 7-day default retention.

use std::io::{BufRead, Write};

use talos_common::{Result, TalosError};

use crate::Span;

/// Export statistics for one file write.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportStats {
    /// Spans written.
    pub spans_written: usize,
    /// Destination path.
    pub path: String,
    /// Export timestamp (RFC 3339).
    pub exported_at: String,
}

fn bad(what: &str) -> TalosError {
    TalosError::Storage(what.to_owned())
}

/// Write spans to `path` as newline-delimited JSON (OTLP-shaped fields).
/// Parent directories are created. Values are re-redacted defensively.
pub fn export_spans_to_file(spans: &[Span], path: &std::path::Path) -> Result<ExportStats> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| bad(&format!("create export dir: {e}")))?;
        }
    }
    let mut file =
        std::fs::File::create(path).map_err(|e| bad(&format!("create export file: {e}")))?;
    let mut written = 0_usize;
    for span in spans {
        let mut safe = span.clone();
        for value in safe.attributes.values_mut() {
            *value = crate::redact_value(value);
        }
        let line = serde_json::to_string(&safe).map_err(|e| bad(&format!("encode span: {e}")))?;
        writeln!(file, "{line}").map_err(|e| bad(&format!("write span: {e}")))?;
        written += 1;
    }
    Ok(ExportStats {
        spans_written: written,
        path: path.to_string_lossy().into_owned(),
        exported_at: talos_common::now_iso(),
    })
}

/// Read back spans exported by [`export_spans_to_file`] (bounded).
pub fn read_spans_from_file(path: &std::path::Path, limit: usize) -> Result<Vec<Span>> {
    let file = std::fs::File::open(path).map_err(|e| bad(&format!("open export: {e}")))?;
    let reader = std::io::BufReader::new(file);
    let mut out = Vec::new();
    for line in reader.lines().take(limit.clamp(1, 10_000)) {
        let line = line.map_err(|e| bad(&format!("read export: {e}")))?;
        if line.trim().is_empty() {
            continue;
        }
        let span: Span =
            serde_json::from_str(&line).map_err(|e| bad(&format!("decode span: {e}")))?;
        out.push(span);
    }
    Ok(out)
}

/// Delete `talos-traces-*.json*` files in `dir` older than `retention`.
/// Returns the number of files removed.
pub fn cleanup_old_exports(dir: &std::path::Path, retention: std::time::Duration) -> usize {
    let entries = std::fs::read_dir(dir)
        .map(|r| r.collect::<Vec<_>>())
        .unwrap_or_default();
    let cutoff = std::time::SystemTime::now()
        .checked_sub(retention)
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
    let mut removed = 0_usize;
    for entry in entries.into_iter().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !(name.starts_with("talos-traces-") && name.ends_with(".json")) {
            continue;
        }
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .map(|t| t < cutoff)
            .unwrap_or(false);
        if old && std::fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SpanCollector, SpanStatus};

    #[test]
    fn export_round_trip() {
        let collector = SpanCollector::with_defaults();
        collector
            .start_span("tool_runtime", "invoke", None, Some("t-1"), None)
            .finish_ok("ok");
        assert_eq!(collector.total_recorded(), 1);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("talos-traces-test.json");
        let stats = export_spans_to_file(&collector.all_spans(), &path).unwrap();
        assert_eq!(stats.spans_written, 1);
        let back = read_spans_from_file(&path, 10).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].status, SpanStatus::Ok);
    }
}
