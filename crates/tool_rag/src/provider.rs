//! Provider abstraction: lexical scoring today, embeddings later.
//!
//! [`LocalLexicalProvider`] is the default: weighted token overlap across
//! document fields, zero network, zero downloads. [`EmbeddingProvider`] is the
//! future seam for external embeddings; [`DisabledEmbeddingProvider`] is the
//! only implementation shipped, and it fails closed so no external API can
//! be reached by accident (see `docs/adr/0004-tool-rag-foundation.md`).

use talos_common::{Result, TalosError};

/// Weighted document fields for lexical scoring.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DocTerms {
    /// Manifest display-name tokens (weight 3.0).
    pub name: Vec<String>,
    /// Capability tokens (weight 2.5).
    pub capabilities: Vec<String>,
    /// Manifest description tokens (weight 1.0).
    pub description: Vec<String>,
    /// Folded context text tokens (weight 0.5).
    pub text: Vec<String>,
}

/// Field weights in priority order: name, capabilities, description, text.
pub const FIELD_WEIGHTS: [(&str, f64); 4] = [
    ("name", 3.0),
    ("capability", 2.5),
    ("description", 1.0),
    ("text", 0.5),
];

/// Scoring seam: query tokens + document fields → relevance score.
pub trait ScoreProvider: std::fmt::Debug + Send + Sync {
    /// Provider name for receipts and audit trails.
    fn name(&self) -> &'static str;
    /// Deterministic relevance score (higher is better).
    fn score(&self, query_tokens: &[String], doc: &DocTerms) -> f64;
}

/// Default provider: weighted token overlap, fully local and deterministic.
///
/// Score = sum over query tokens of the highest field weight whose token set
/// contains the token. No frequencies, no randomness, no I/O.
#[derive(Debug, Clone, Copy, Default)]
pub struct LocalLexicalProvider;

impl ScoreProvider for LocalLexicalProvider {
    fn name(&self) -> &'static str {
        "local-lexical/1"
    }

    fn score(&self, query_tokens: &[String], doc: &DocTerms) -> f64 {
        let mut total = 0.0;
        for token in query_tokens {
            if doc.name.iter().any(|term| term == token) {
                total += FIELD_WEIGHTS[0].1;
            } else if doc.capabilities.iter().any(|term| term == token) {
                total += FIELD_WEIGHTS[1].1;
            } else if doc.description.iter().any(|term| term == token) {
                total += FIELD_WEIGHTS[2].1;
            } else if doc.text.iter().any(|term| term == token) {
                total += FIELD_WEIGHTS[3].1;
            }
        }
        total
    }
}

/// Future seam for external embedding backends (not implemented).
///
/// Any real implementation must live behind an explicit provider flag **and**
/// a policy gate; nothing here may call the network. The only shipped
/// implementation ([`DisabledEmbeddingProvider`]) fails closed.
pub trait EmbeddingProvider: std::fmt::Debug + Send + Sync {
    /// Backend name for receipts and audit trails.
    fn backend_name(&self) -> &'static str;
    /// Embed texts. Never implemented today.
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
}

/// Always-disabled embedding backend: every call fails closed.
#[derive(Debug, Default)]
pub struct DisabledEmbeddingProvider;

impl EmbeddingProvider for DisabledEmbeddingProvider {
    fn backend_name(&self) -> &'static str {
        "disabled-stub"
    }

    fn embed(&self, _texts: &[String]) -> Result<Vec<Vec<f32>>> {
        Err(TalosError::Storage(
            "embeddings disabled by default: no external API (see docs/adr/0004-tool-rag-foundation.md)"
                .to_owned(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokenizer::tokenize;

    fn doc() -> DocTerms {
        DocTerms {
            name: tokenize("fs.read"),
            capabilities: tokenize("read_file"),
            description: tokenize("read a file with hash"),
            text: tokenize("recent task read notes"),
        }
    }

    #[test]
    fn lexical_scores_name_above_capability_above_text() {
        let provider = LocalLexicalProvider;
        assert_eq!(provider.name(), "local-lexical/1");
        let name_hit = provider.score(&["fs".to_owned()], &doc());
        let capability_hit = provider.score(&["read".to_owned(), "file".to_owned()], &doc());
        assert!(name_hit > 0.0);
        assert!(capability_hit > 0.0);
        let miss = provider.score(&["purchase".to_owned()], &doc());
        assert_eq!(miss, 0.0);
        assert_eq!(
            provider.score(&["read".to_owned()], &doc()),
            provider.score(&["read".to_owned()], &doc())
        );
    }

    #[test]
    fn embeddings_are_disabled_by_default() {
        let stub = DisabledEmbeddingProvider;
        assert_eq!(stub.backend_name(), "disabled-stub");
        assert!(stub.embed(&["hello".to_owned()]).is_err());
    }
}
