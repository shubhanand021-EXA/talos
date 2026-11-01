//! Process-wide default collector: instrumented crates record spans here
//! without plumbing a handle through every call chain.
//!
//! When no global collector is installed, [`record_now`] is a no-op, so
//! instrumentation has zero observable effect outside the desktop shell
//! (which installs one at startup) and tests.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::{is_allowed_attribute, redact_value, span_name, Span, SpanKind, SpanStatus};

static GLOBAL: OnceLock<SpanCollectorHandle> = OnceLock::new();

/// Cloneable handle to the process-wide collector.
#[derive(Debug, Clone)]
pub struct SpanCollectorHandle {
    inner: crate::SpanCollector,
}

impl SpanCollectorHandle {
    /// Wrap a collector as the global handle.
    #[must_use]
    pub fn new(collector: crate::SpanCollector) -> Self {
        Self { inner: collector }
    }
}

/// Install the process-wide collector (first call wins; later calls are
/// no-ops so tests cannot clobber the shell's collector).
pub fn set_global_collector(collector: crate::SpanCollector) {
    let _ = GLOBAL.set(SpanCollectorHandle::new(collector));
}

/// Borrow the global collector, if installed.
#[must_use]
pub fn global_collector() -> Option<crate::SpanCollector> {
    GLOBAL.get().map(|handle| handle.inner.clone())
}

/// Record a finished span on the global collector (no-op when unset).
///
/// `extra` attributes are privacy-filtered (non-allowlisted keys dropped,
/// values redacted) before recording.
#[allow(clippy::too_many_arguments)]
pub fn record_now(
    component: &str,
    operation: &str,
    task_id: Option<&str>,
    agent_id: Option<&str>,
    status: SpanStatus,
    status_message: &str,
    duration_ms: i64,
    extra: &[(&str, serde_json::Value)],
) {
    let Some(collector) = global_collector() else {
        return;
    };
    let now = chrono::Utc::now();
    let start = now - chrono::Duration::milliseconds(duration_ms.max(0));
    let mut attributes = HashMap::new();
    attributes.insert("talos.component".to_owned(), serde_json::json!(component));
    attributes.insert("talos.operation".to_owned(), serde_json::json!(operation));
    if let Some(task) = task_id {
        attributes.insert("talos.task_id".to_owned(), serde_json::json!(task));
    }
    if let Some(agent) = agent_id {
        attributes.insert("talos.agent_id".to_owned(), serde_json::json!(agent));
    }
    attributes.insert("duration_ms".to_owned(), serde_json::json!(duration_ms));
    for (key, value) in extra {
        if is_allowed_attribute(key) {
            attributes.insert((*key).to_owned(), redact_value(value));
        } else {
            collector.add_privacy_drops(1);
        }
    }
    collector.record(Span {
        trace_id: crate::TraceContext::new_trace_id(),
        span_id: crate::TraceContext::new_span_id(),
        parent_span_id: None,
        name: span_name(component, operation),
        component: component.to_owned(),
        operation: operation.to_owned(),
        kind: SpanKind::Internal,
        start_time: start.to_rfc3339(),
        end_time: now.to_rfc3339(),
        duration_ms,
        status,
        status_message: status_message.chars().take(256).collect(),
        attributes,
    });
}
