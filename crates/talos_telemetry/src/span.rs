//! Span model: OTLP-shaped, privacy-safe by construction.

use std::collections::HashMap;

/// Span status (OTLP `Status.code` equivalent).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SpanStatus {
    /// Operation succeeded.
    Ok,
    /// Operation failed.
    Error,
}

/// Span kind (subset of OTLP `SpanKind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SpanKind {
    /// Internal operation (default).
    Internal,
    /// Outgoing IPC call.
    Client,
    /// Incoming IPC handler.
    Server,
}

/// A single finished span.
///
/// Only privacy-safe attributes are permitted: keys must either start with
/// `talos.` or appear in the small numeric allowlist (`duration_ms`,
/// `tokens.*`, `cost.*`, `count.*`, `latency.*`, ...). See
/// [`crate::is_allowed_attribute`].
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Span {
    /// W3C trace id (32 lowercase hex chars).
    pub trace_id: String,
    /// This span's id (16 lowercase hex chars).
    pub span_id: String,
    /// Parent span id, if any.
    pub parent_span_id: Option<String>,
    /// `talos.{component}.{operation}`.
    pub name: String,
    /// Component that produced the span (e.g. `policy_broker`).
    pub component: String,
    /// Operation within the component (e.g. `evaluate`).
    pub operation: String,
    /// OTLP-style span kind.
    pub kind: SpanKind,
    /// Start timestamp (RFC 3339).
    pub start_time: String,
    /// End timestamp (RFC 3339).
    pub end_time: String,
    /// Wall-clock duration in milliseconds.
    pub duration_ms: i64,
    /// Outcome.
    pub status: SpanStatus,
    /// Short machine-readable outcome (`ok`, `policy_denied`, `timeout`, ...).
    pub status_message: String,
    /// Privacy-safe attributes only.
    pub attributes: HashMap<String, serde_json::Value>,
}

impl Span {
    /// Build the canonical span name for a component + operation.
    #[must_use]
    pub fn named(component: &str, operation: &str) -> String {
        span_name(component, operation)
    }
}

/// Canonical span name: `talos.{component}.{operation}`.
///
/// Components and operations are lowercased, trimmed, and sanitized to
/// `[a-z0-9_.]`; anything else becomes `_`. Empty inputs fall back to
/// `unknown`.
#[must_use]
pub fn span_name(component: &str, operation: &str) -> String {
    fn sanitize(input: &str) -> String {
        let cleaned: String = input
            .trim()
            .to_lowercase()
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let cleaned = cleaned.trim_matches('_').to_owned();
        if cleaned.is_empty() {
            "unknown".to_owned()
        } else {
            cleaned
        }
    }
    format!("talos.{}.{}", sanitize(component), sanitize(operation))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn span_names_follow_convention() {
        assert_eq!(
            span_name("policy_broker", "evaluate"),
            "talos.policy_broker.evaluate"
        );
        assert_eq!(
            span_name("LLM-Gateway", "Model Call"),
            "talos.llm_gateway.model_call"
        );
        assert_eq!(span_name("", ""), "talos.unknown.unknown");
    }
}
