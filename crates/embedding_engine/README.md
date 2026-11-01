# embedding_engine

Local-first text embeddings: deterministic, offline, no downloads.

```sh
cargo test -p embedding_engine
```

- `HashEmbedding` (default, 384 dims): hashing-trick bag-of-tokens, L2-normalized.
  `embed` / `embed_batch` are pure functions of the input text.
- `TfIdfVectorizer`: corpus-fitted weighting over the same tokenizer.
- `cosine_similarity`, `l2_norm` helpers; `model_dir_status()` probes
  `.talos/models/` and reports the hash fallback (see ADR 0007).
