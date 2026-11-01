//! W3C-compatible trace context propagation across IPC boundaries.
//!
//! A [`TraceContext`] carries `{trace_id, span_id}` hex identifiers. It
//! serializes to/from the `traceparent` header format
//! (`00-{trace_id}-{span_id}-01`) so future OTLP bridges interoperate
//! without extra mapping.

/// Trace context propagated across IPC boundaries.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceContext {
    /// 32 lowercase hex chars.
    pub trace_id: String,
    /// 16 lowercase hex chars (parent span).
    pub span_id: String,
}

impl TraceContext {
    /// Generate a fresh root context (random ids).
    #[must_use]
    pub fn new_root() -> Self {
        Self {
            trace_id: Self::new_trace_id(),
            span_id: Self::new_span_id(),
        }
    }

    /// Generate a fresh 16-hex-char span id.
    #[must_use]
    pub fn new_span_id() -> String {
        let bytes = uuid::Uuid::new_v4();
        let hex = format!("{:032x}", bytes.as_u128());
        hex[..16].to_owned()
    }

    /// Generate a fresh 32-hex-char trace id.
    #[must_use]
    pub fn new_trace_id() -> String {
        let hi = uuid::Uuid::new_v4().as_u128();
        let lo = uuid::Uuid::new_v4().as_u128();
        format!("{hi:032x}{lo:032x}")[..32].to_owned()
    }

    /// Child context sharing this trace with a fresh span id.
    #[must_use]
    pub fn child(&self) -> Self {
        Self {
            trace_id: self.trace_id.clone(),
            span_id: Self::new_span_id(),
        }
    }
}

/// W3C `traceparent` header helpers.
#[derive(Debug)]
pub struct Traceparent;

impl Traceparent {
    /// Encode a context as `00-{trace_id}-{span_id}-01`.
    #[must_use]
    pub fn encode(ctx: &TraceContext) -> String {
        format!("00-{}-{}-01", ctx.trace_id, ctx.span_id)
    }

    /// Decode a `traceparent` header back into a context.
    #[must_use]
    pub fn decode(header: &str) -> Option<TraceContext> {
        let parts: Vec<&str> = header.trim().split('-').collect();
        if parts.len() != 4 || parts[0] != "00" {
            return None;
        }
        let (trace_id, span_id) = (parts[1], parts[2]);
        if trace_id.len() != 32
            || span_id.len() != 16
            || !trace_id.chars().all(|c| c.is_ascii_hexdigit())
            || !span_id.chars().all(|c| c.is_ascii_hexdigit())
        {
            return None;
        }
        Some(TraceContext {
            trace_id: trace_id.to_lowercase(),
            span_id: span_id.to_lowercase(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traceparent_round_trip() {
        let ctx = TraceContext {
            trace_id: "0af7651916cd43dd8448eb211c80319c".to_owned(),
            span_id: "b7ad6b7169203331".to_owned(),
        };
        let header = Traceparent::encode(&ctx);
        assert_eq!(
            header,
            "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01"
        );
        assert_eq!(Traceparent::decode(&header), Some(ctx));
        assert_eq!(Traceparent::decode("bogus"), None);
    }

    #[test]
    fn generated_ids_have_correct_shape() {
        let trace = TraceContext::new_trace_id();
        let span = TraceContext::new_span_id();
        assert_eq!(trace.len(), 32);
        assert_eq!(span.len(), 16);
        assert!(trace.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
