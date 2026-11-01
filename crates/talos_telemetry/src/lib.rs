//! Enterprise observability for Talos: OpenTelemetry-compatible tracing
//! with a local-first retention policy.
//!
//! Design decisions (privacy-first, offline-safe):
//! - No external telemetry endpoints. Spans are buffered in memory and
//!   exported to a local JSON file (one JSON object per line, OTLP-shaped
//!   fields) only. An optional OTLP/HTTP collector endpoint is supported via
//!   explicit configuration but is **disabled by default** and never
//!   contacted unless the operator opts in.
//! - Span naming convention: `talos.{component}.{operation}`
//!   (e.g. `talos.policy_broker.evaluate`).
//! - Every span carries only privacy-safe attributes (`talos.task_id`,
//!   `talos.agent_id`, `talos.component`, `talos.operation`, durations,
//!   status). Raw prompts, file contents, secrets, screenshots, and personal
//!   data are never recorded; [`redact_value`] scrubs accidental leaks
//!   before export.
//! - Trace context propagation uses W3C `traceparent`-compatible
//!   `{trace_id, span_id}` hex identifiers carried across IPC boundaries in
//!   [`TraceContext`].
//!
//! The collector is intentionally dependency-free (no `opentelemetry` crates:
//! they would pull an OTLP network exporter into the default build, which
//! contradicts the local-first privacy rule). The JSON export is shaped so a
//! future OTLP bridge can translate it field-for-field.

mod collector;
mod context;
mod exporter;
mod global;
mod privacy;
mod span;

pub use collector::{CollectorConfig, SpanCollector, SpanSummary, TimedSpan};
pub use context::{TraceContext, Traceparent};
pub use exporter::{cleanup_old_exports, export_spans_to_file, read_spans_from_file, ExportStats};
pub use global::{global_collector, record_now, set_global_collector};
pub use privacy::{contains_sensitive_pattern, is_allowed_attribute, redact_value};
pub use span::{span_name, Span, SpanKind, SpanStatus};
