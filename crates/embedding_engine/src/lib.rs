//! Local-first text embeddings: deterministic, offline, no downloads.
//!
//! The engine ships two pure-Rust backends and zero native dependencies:
//!
//! - [`HashEmbedding`] (default, 384 dims): hashing-trick bag-of-tokens
//!   projection, L2-normalized. Deterministic: the same text always yields
//!   the same vector on every machine, with no model file.
//! - [`TfIdfVectorizer`]: fitted term weighting over a caller-supplied
//!   corpus, L2-normalized. Deterministic for a fixed corpus.
//!
//! A production ONNX model (e.g. all-MiniLM-L6-v2, also 384 dims) is an
//! explicit non-goal for this batch: loading it would require the `ort`
//! native runtime plus a ~90MB download, which breaks the offline,
//! deterministic CI gate. [`model_dir_status`] probes `.talos/models/` so a
//! future ONNX backend can take over the same [`EMBEDDING_DIMS`] shape
//! without migrating stored vectors; until then every call falls back to
//! the hash backend with a caller-visible warning string.

use std::collections::HashMap;

use sha2::{Digest, Sha256};

/// Embedding width (matches all-MiniLM-L6-v2 so a future ONNX backend can
/// reuse stored vectors without migration).
pub const EMBEDDING_DIMS: usize = 384;

/// Small stopword list: function words carry no recall signal.
const STOPWORDS: &[&str] = &[
    "the", "and", "for", "with", "from", "that", "this", "have", "were", "been", "are", "was",
    "will", "would", "there", "their", "what", "when", "which", "into", "than", "then", "them",
    "they", "about", "after", "also", "other", "such", "only", "over", "under", "between",
    "through", "during", "each", "both", "few", "more", "most", "some", "these", "those",
];

/// Split text into lowercase alphanumeric tokens (length >= 2).
#[must_use]
pub fn tokenize_embed(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter_map(|piece| {
            let token = piece.to_ascii_lowercase();
            if token.len() >= 2 && !STOPWORDS.contains(&token.as_str()) {
                Some(token)
            } else {
                None
            }
        })
        .collect()
}

/// L2 norm of a dense vector.
#[must_use]
pub fn l2_norm(vector: &[f32]) -> f32 {
    vector.iter().map(|x| x * x).sum::<f32>().sqrt()
}

/// Cosine similarity in [-1, 1] (0 when either side is all zeros).
#[must_use]
pub fn cosine_similarity(left: &[f32], right: &[f32]) -> f32 {
    let denom = l2_norm(left) * l2_norm(right);
    if denom <= f32::EPSILON {
        return 0.0;
    }
    let dot: f32 = left.iter().zip(right.iter()).map(|(a, b)| a * b).sum();
    (dot / denom).clamp(-1.0, 1.0)
}

fn normalize(vector: &mut [f32]) {
    let norm = l2_norm(vector);
    if norm > f32::EPSILON {
        for value in vector.iter_mut() {
            *value /= norm;
        }
    }
}

/// Hashing-trick text embedding: deterministic, order-insensitive,
/// L2-normalized, zero native dependencies.
///
/// Each token votes `+1`/`-1` (from its SHA-256) into one of 384 buckets;
/// the result is L2-normalized. Similar texts share tokens and therefore
/// score higher under cosine similarity.
#[derive(Debug, Clone, Copy, Default)]
pub struct HashEmbedding;

impl HashEmbedding {
    /// Embed one text (empty text yields the zero vector).
    #[must_use]
    pub fn embed(&self, text: &str) -> Vec<f32> {
        let mut vector = vec![0.0_f32; EMBEDDING_DIMS];
        for token in tokenize_embed(text) {
            let digest = Sha256::digest(token.as_bytes());
            let lane = u64::from_le_bytes(digest[0..8].try_into().expect("sha256 is 32 bytes"))
                as usize
                % EMBEDDING_DIMS;
            let sign = if digest[8] & 1 == 0 { 1.0 } else { -1.0 };
            vector[lane] += sign;
        }
        normalize(&mut vector);
        vector
    }

    /// Embed a batch (pure function of the inputs; order-preserving).
    #[must_use]
    pub fn embed_batch(&self, texts: &[String]) -> Vec<Vec<f32>> {
        texts.iter().map(|text| self.embed(text)).collect()
    }
}

/// Which local backend produced a vector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddingBackend {
    /// Hashing-trick backend (default).
    Hash384,
    /// Corpus-fitted TF-IDF backend.
    TfIdf,
}

impl EmbeddingBackend {
    /// Stable backend name for receipts and audit trails.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Hash384 => "local-hash-384/1",
            Self::TfIdf => "local-tfidf/1",
        }
    }
}

/// Embed one text with the default backend (deterministic, offline).
#[must_use]
pub fn embed(text: &str) -> Vec<f32> {
    HashEmbedding.embed(text)
}

/// Embed a batch with the default backend (order-preserving).
#[must_use]
pub fn embed_batch(texts: &[String]) -> Vec<Vec<f32>> {
    HashEmbedding.embed_batch(texts)
}

/// Status of the on-disk model directory (future ONNX backend seam).
///
/// Today this always reports `model_loaded: false`: no model is bundled and
/// nothing is downloaded. The probe exists so callers can surface the exact
/// fallback warning instead of silently degrading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelDirStatus {
    /// Directory that was probed.
    pub dir: String,
    /// Whether a `*.onnx` file was found (informational only).
    pub model_present: bool,
    /// Always false until an ONNX runtime backend ships.
    pub model_loaded: bool,
    /// Active backend name.
    pub backend: &'static str,
    /// Caller-visible fallback note.
    pub warning: String,
}

/// Probe `.talos/models/` for a future ONNX model (never loads anything).
#[must_use]
pub fn model_dir_status() -> ModelDirStatus {
    let dir = std::env::var("TALOS_MODEL_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from(".talos/models"));
    let model_present = std::fs::read_dir(&dir).is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("onnx"))
        })
    });
    ModelDirStatus {
        dir: dir.to_string_lossy().into_owned(),
        model_present,
        model_loaded: false,
        backend: EmbeddingBackend::Hash384.name(),
        warning: "no ONNX runtime bundled; using deterministic local hash embeddings (384 dims)"
            .to_owned(),
    }
}

/// Corpus-fitted TF-IDF vectorizer (deterministic for a fixed corpus).
///
/// Vocabulary is token-sorted, so identical corpora always yield identical
/// indices. Vectors are L2-normalized; unseen tokens are ignored.
#[derive(Debug, Clone, Default)]
pub struct TfIdfVectorizer {
    /// Token → dense index (token-sorted).
    vocab: HashMap<String, usize>,
    /// Token → document frequency.
    doc_freq: HashMap<String, usize>,
    /// Documents fitted.
    doc_count: usize,
}

impl TfIdfVectorizer {
    /// Create an empty vectorizer.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Fit on a corpus (replaces any previous fit).
    pub fn fit(&mut self, docs: &[String]) {
        self.vocab.clear();
        self.doc_freq.clear();
        self.doc_count = docs.len();
        let mut seen: Vec<std::collections::HashSet<String>> = Vec::new();
        for doc in docs {
            seen.push(tokenize_embed(doc).into_iter().collect());
        }
        let mut tokens: Vec<String> = seen.iter().flatten().cloned().collect();
        tokens.sort();
        tokens.dedup();
        for (index, token) in tokens.into_iter().enumerate() {
            self.vocab.insert(token.clone(), index);
        }
        for terms in &seen {
            for term in terms {
                *self.doc_freq.entry(term.clone()).or_insert(0) += 1;
            }
        }
    }

    /// Vocabulary size.
    #[must_use]
    pub fn vocab_size(&self) -> usize {
        self.vocab.len()
    }

    /// Transform one text into a dense L2-normalized TF-IDF vector.
    #[must_use]
    pub fn transform(&self, text: &str) -> Vec<f32> {
        let mut vector = vec![0.0_f32; self.vocab.len()];
        if self.vocab.is_empty() {
            return vector;
        }
        let mut counts: HashMap<String, f32> = HashMap::new();
        for token in tokenize_embed(text) {
            if self.vocab.contains_key(&token) {
                *counts.entry(token).or_insert(0.0) += 1.0;
            }
        }
        for (token, count) in &counts {
            if let Some(&index) = self.vocab.get(token) {
                let doc_freq = self.doc_freq.get(token).copied().unwrap_or(1) as f32;
                let idf = ((self.doc_count as f32 + 1.0) / (doc_freq + 1.0)).ln() + 1.0;
                vector[index] = count * idf;
            }
        }
        normalize(&mut vector);
        vector
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embed_is_deterministic() {
        let first = embed("read a file with hash");
        let second = embed("read a file with hash");
        assert_eq!(first, second);
        assert_eq!(first.len(), EMBEDDING_DIMS);
    }

    #[test]
    fn embeddings_are_l2_normalized() {
        let vector = embed("hello world tokens");
        let norm = l2_norm(&vector);
        assert!((norm - 1.0).abs() < 1e-5, "norm was {norm}");
        let empty = embed("");
        assert!(empty.iter().all(|&x| x == 0.0));
        assert_eq!(cosine_similarity(&empty, &vector), 0.0);
    }

    #[test]
    fn similar_texts_score_above_dissimilar() {
        let a = embed("read file contents from disk");
        let b = embed("read file contents with hash");
        let c = embed("purchase checkout browser button");
        let similar = cosine_similarity(&a, &b);
        let dissimilar = cosine_similarity(&a, &c);
        assert!(similar > dissimilar, "{similar} <= {dissimilar}");
        assert!((cosine_similarity(&a, &a) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn batch_matches_single() {
        let texts = vec!["alpha beta".to_owned(), "gamma delta".to_owned()];
        assert_eq!(
            embed_batch(&texts),
            vec![embed(&texts[0]), embed(&texts[1])]
        );
    }

    #[test]
    fn tfidf_fit_transform_is_deterministic() {
        let corpus = vec![
            "read file contents".to_owned(),
            "browser click button".to_owned(),
            "read file hash".to_owned(),
        ];
        let mut first = TfIdfVectorizer::new();
        first.fit(&corpus);
        let mut second = TfIdfVectorizer::new();
        second.fit(&corpus);
        assert_eq!(first.transform("read file"), second.transform("read file"));
        assert_eq!(first.vocab_size(), second.vocab_size());
        // Shared terms score above disjoint terms.
        let query = first.transform("read file");
        let close = first.transform("read file contents");
        let far = first.transform("browser click button");
        assert!(cosine_similarity(&query, &close) > cosine_similarity(&query, &far));
    }

    #[test]
    fn model_status_reports_hash_fallback() {
        let status = model_dir_status();
        assert!(!status.model_loaded);
        assert_eq!(status.backend, "local-hash-384/1");
        assert!(!status.warning.is_empty());
    }

    #[test]
    fn embedding_batch_is_fast() {
        let texts: Vec<String> = (0..100)
            .map(|i| format!("task {i} read file hash"))
            .collect();
        let started = std::time::Instant::now();
        let vectors = embed_batch(&texts);
        assert_eq!(vectors.len(), 100);
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
    }
}
