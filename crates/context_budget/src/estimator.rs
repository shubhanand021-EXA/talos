//! Deterministic local token estimator: char-fallback, no external calls.
//!
//! Rule: `tokens = ceil(chars / 4)`, `0` for empty text. Character count uses
//! Unicode scalar values (`str::chars`), so output is identical on every
//! platform for the same input. Callers must never call an LLM or tokenizer
//! service here; determinism is load-bearing for the proof hash.

/// Estimate tokens for free text (char-fallback, deterministic).
#[must_use]
pub fn estimate_tokens(text: &str) -> u64 {
    let chars = text.chars().count() as u64;
    if chars == 0 {
        0
    } else {
        chars.div_ceil(4)
    }
}

/// Estimate tokens for one segment body.
#[must_use]
pub fn segment_tokens(body: &str) -> u64 {
    estimate_tokens(body)
}

/// Estimate tokens for a whole slice of bodies.
#[must_use]
pub fn bodies_tokens<'a>(bodies: impl IntoIterator<Item = &'a str>) -> u64 {
    bodies.into_iter().map(estimate_tokens).sum()
}

/// Estimate tokens for a full snapshot (sum over segment bodies).
#[must_use]
pub fn snapshot_tokens(snapshot: &crate::model::ContextSnapshot) -> u64 {
    snapshot
        .segments
        .iter()
        .map(|segment| estimate_tokens(&segment.body))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimator_is_char_fallback_and_deterministic() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("a"), 1);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
        assert_eq!(estimate_tokens("abcdabcd"), 2);
        // Same input, same output — twice.
        assert_eq!(
            estimate_tokens("hello world"),
            estimate_tokens("hello world")
        );
        // Unicode counts scalars, not bytes.
        assert_eq!(estimate_tokens("éééé"), 1);
    }

    #[test]
    fn snapshot_sums_segment_bodies() {
        let snapshot = crate::model::ContextSnapshot {
            task_id: "task-1".to_owned(),
            session_id: "session-local".to_owned(),
            segments: vec![
                crate::model::ContextSegment {
                    id: "a".to_owned(),
                    layer: crate::model::ContextLayer::System,
                    body: "abcd".to_owned(),
                    protected: true,
                    safety_critical: false,
                    schema_type: None,
                    evidence_refs: vec![],
                    compensation_refs: vec![],
                    policy_refs: vec![],
                    action_constraints: vec![],
                },
                crate::model::ContextSegment {
                    id: "b".to_owned(),
                    layer: crate::model::ContextLayer::Scratchpad,
                    body: "abcde".to_owned(),
                    protected: false,
                    safety_critical: false,
                    schema_type: None,
                    evidence_refs: vec![],
                    compensation_refs: vec![],
                    policy_refs: vec![],
                    action_constraints: vec![],
                },
            ],
        };
        assert_eq!(snapshot_tokens(&snapshot), 3);
    }
}
