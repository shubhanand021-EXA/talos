//! Deterministic tokenization: lowercase ASCII-alphanumeric tokens, no
//! external calls, no network, no model downloads.
//!
//! Rules: split on every non-`[a-z0-9]` boundary, drop tokens shorter than 2
//! chars, drop a fixed English stopword list. Output is a sorted unique token
//! set plus per-token counts, so ranking is identical on every platform for
//! the same input.

use std::collections::{BTreeMap, BTreeSet};

/// Fixed English stopwords (deterministic, versioned with this module).
pub const STOPWORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "by", "can", "could", "did", "do", "does", "for",
    "from", "in", "is", "it", "its", "no", "not", "of", "on", "or", "should", "so", "such", "than",
    "that", "the", "these", "this", "those", "then", "to", "use", "used", "using", "will", "with",
    "would",
];

/// Raw token split: lowercase, alphanumeric runs, length >= 2 (stopwords kept).
#[must_use]
pub fn split_tokens(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            current.push(ch.to_ascii_lowercase());
        } else if !current.is_empty() {
            if current.len() >= 2 {
                tokens.push(std::mem::take(&mut current));
            } else {
                current.clear();
            }
        }
    }
    if current.len() >= 2 {
        tokens.push(current);
    }
    tokens
}

/// Deterministic query/document tokens: split, drop stopwords, sort, dedupe.
#[must_use]
pub fn tokenize(text: &str) -> Vec<String> {
    let stopwords: BTreeSet<&str> = STOPWORDS.iter().copied().collect();
    let mut tokens: Vec<String> = split_tokens(text)
        .into_iter()
        .filter(|token| !stopwords.contains(token.as_str()))
        .collect();
    tokens.sort();
    tokens.dedup();
    tokens
}

/// Token counts for weighted scoring (stopwords dropped, deterministic order).
#[must_use]
pub fn token_counts(text: &str) -> BTreeMap<String, u64> {
    let stopwords: BTreeSet<&str> = STOPWORDS.iter().copied().collect();
    let mut counts = BTreeMap::new();
    for token in split_tokens(text) {
        if stopwords.contains(token.as_str()) {
            continue;
        }
        *counts.entry(token).or_insert(0) += 1;
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenization_is_deterministic_and_local() {
        assert_eq!(tokenize(""), Vec::<String>::new());
        assert_eq!(
            tokenize("Read a FILE!"),
            vec!["file".to_owned(), "read".to_owned()]
        );
        // Punctuation splits; short tokens drop; stopwords drop.
        assert_eq!(
            tokenize("fs.read the file, x2!"),
            vec![
                "file".to_owned(),
                "fs".to_owned(),
                "read".to_owned(),
                "x2".to_owned()
            ]
        );
        assert_eq!(tokenize("Read a FILE!"), tokenize("read A file!"));
        // No network, no filesystem: pure function of the input.
        assert_eq!(tokenize("a"), Vec::<String>::new());
    }

    #[test]
    fn counts_track_repetition_deterministically() {
        let counts = token_counts("click click button");
        assert_eq!(counts.get("click"), Some(&2));
        assert_eq!(counts.get("button"), Some(&1));
        assert!(!counts.contains_key("a"));
    }
}
