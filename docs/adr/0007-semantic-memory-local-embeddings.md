# ADR 0007: Semantic memory with local embeddings (hash now, ONNX later)

Date: 2026-11-01
Status: Accepted

## Context

Talos Batch 3 upgrades memory from SQLite FTS5-style lexical search to hybrid
retrieval (Grok §2.3, Meta AI §2.2, Perplexity §2.5, Manus §2.5, ChatGPT §2.3).
The mission names `ort` (ONNX Runtime, e.g. all-MiniLM-L6-v2) as the primary
embedding engine with a TF-IDF fallback.

## Decision

Ship pure-Rust deterministic embeddings now; defer ONNX.

- `embedding_engine` implements a 384-dim hashing-trick backend (L2-normalized,
  `local-hash-384/1`) plus a corpus-fitted TF-IDF vectorizer. No native
  dependencies, no downloads, no network.
- `vector_store` keeps 384-dim `f32` blobs in the existing `EvidenceDb`
  (SQLite BLOB + brute-force cosine in Rust).
- `model_dir_status()` probes `.talos/models/` for a future `*.onnx` file and
  reports `model_loaded: false` with a caller-visible fallback warning.

## Rationale

1. `ort` pulls prebuilt ONNX Runtime binaries at build time. That breaks the
   offline, deterministic CI gate (Batch 2 audit pins, Windows runners) and
   adds ~90MB to every install for a marginal quality gain on short
   ledger text.
2. 384 dims match all-MiniLM-L6-v2 exactly, so stored vectors migrate without
   reindexing when an ONNX backend lands: only `embed()` changes.
3. Ledger text (claims, summaries, task titles) is short and keyword-dense;
   hashing + hybrid rerank (recency, verified) already separates recall from
   noise at this scale (<10k entries).

## Consequences

- `backend` fields report `local-hash-384/1`; the UI shows the hash-fallback
  warning honestly instead of implying a model.
- A future ADR can introduce `ort` behind the same `EMBEDDING_DIMS` contract;
  `vector_store` needs no migration.
- Alternatives considered: `sqlite-vss`/`usearch` HNSW (native libs, same
  offline-CI problem); remote embedding APIs (rejected: local-first is a
  hard requirement).
