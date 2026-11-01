//! Local token estimation with a recorded estimator identity.
//!
//! OpenAI-family models use tiktoken `cl100k` BPE counts; every other model
//! (and any offline failure) falls back to the deterministic char/4
//! heuristic inherited from `context_budget`. The active estimator is stamped
//! on every assembly so counts stay auditable.

use std::sync::OnceLock;

/// Which estimator produced a count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EstimatorKind {
    /// tiktoken `cl100k` BPE (OpenAI-family models).
    TiktokenCl100k,
    /// Deterministic `ceil(chars/4)` fallback.
    CharHeuristic,
}

impl EstimatorKind {
    /// Stable wire name recorded on assemblies.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            EstimatorKind::TiktokenCl100k => "tiktoken-cl100k",
            EstimatorKind::CharHeuristic => "char-heuristic-v1",
        }
    }

    /// Pick by model id prefix (`gpt-`/`o1`/`o3` → BPE).
    #[must_use]
    pub fn for_model(model: &str) -> Self {
        let lower = model.to_lowercase();
        if lower.starts_with("gpt-")
            || lower.starts_with("o1")
            || lower.starts_with("o3")
            || lower.starts_with("chatgpt")
        {
            EstimatorKind::TiktokenCl100k
        } else {
            EstimatorKind::CharHeuristic
        }
    }
}

fn cl100k() -> Option<&'static tiktoken_rs::CoreBPE> {
    static ENCODING: OnceLock<Option<tiktoken_rs::CoreBPE>> = OnceLock::new();
    ENCODING
        .get_or_init(|| tiktoken_rs::cl100k_base().ok())
        .as_ref()
}

/// Count tokens for `text` with the model-appropriate estimator.
/// Falls back to the char heuristic when BPE is unavailable (offline).
#[must_use]
pub fn estimate_tokens(model: &str, text: &str) -> (u32, EstimatorKind) {
    if EstimatorKind::for_model(model) == EstimatorKind::TiktokenCl100k {
        if let Some(bpe) = cl100k() {
            let count = bpe.encode_with_special_tokens(text).len();
            return (
                count.min(u32::MAX as usize) as u32,
                EstimatorKind::TiktokenCl100k,
            );
        }
    }
    (char_estimate(text), EstimatorKind::CharHeuristic)
}

/// Deterministic `ceil(chars/4)` fallback (matches `context_budget`).
#[must_use]
pub fn char_estimate(text: &str) -> u32 {
    (text.chars().count() as u64)
        .div_ceil(4)
        .min(u64::from(u32::MAX)) as u32
}

/// Force the fallback path (tests + offline callers).
#[must_use]
pub fn estimate_tokens_fallback(text: &str) -> (u32, EstimatorKind) {
    (char_estimate(text), EstimatorKind::CharHeuristic)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimator_selection_by_model() {
        assert_eq!(
            EstimatorKind::for_model("gpt-4o-mini"),
            EstimatorKind::TiktokenCl100k
        );
        assert_eq!(
            EstimatorKind::for_model("o3-mini"),
            EstimatorKind::TiktokenCl100k
        );
        assert_eq!(
            EstimatorKind::for_model("llama3"),
            EstimatorKind::CharHeuristic
        );
        assert_eq!(
            EstimatorKind::for_model("claude-sonnet-4-20250514"),
            EstimatorKind::CharHeuristic
        );
    }

    #[test]
    fn counts_are_deterministic() {
        let text = "Hello, world! Hello, world!";
        let (a, _) = estimate_tokens("gpt-4o-mini", text);
        let (b, _) = estimate_tokens("gpt-4o-mini", text);
        assert_eq!(a, b);
        let (c, kind) = estimate_tokens_fallback(text);
        assert_eq!(kind, EstimatorKind::CharHeuristic);
        assert_eq!(c, char_estimate(text));
        assert!(a > 0);
    }

    #[test]
    fn fallback_matches_context_budget_heuristic() {
        assert_eq!(char_estimate(""), 0);
        assert_eq!(char_estimate("abcd"), 1);
        assert_eq!(char_estimate("abcde"), 2);
    }
}
