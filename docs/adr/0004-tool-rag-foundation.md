# ADR 0004: Tool-RAG local-first foundation

Date: 2026-10-31
Status: accepted

## Context

Talos must learn to route intent to the correct tool. Before external
embeddings exist, it needs a deterministic local retrieval foundation over
tool manifests, invocation receipts, evidence records, and task history —
with no network calls, no downloads, and auditable scores.

## Decision

**Rust-first deterministic retrieval** (`crates/tool_rag`), contracts-first
wire shapes, one read-only IPC command, UI panel on the Tools page:

1. **SQLite index, normalized tables, no FTS5** (`index.rs`): `rag_tools`
   (one document per manifest plus folded task/evidence/compensation
   context), `rag_terms` (exact token → tool + field + weight postings),
   `rag_receipts` (per-tool success/failed/denied/disabled counters).
   FTS5 was evaluated and rejected: its native tokenizer varies across
   platforms and its BM25 internals are not auditable; exact-match token
   tables give byte-deterministic ordering.
2. **Deterministic tokenizer** (`tokenizer.rs`): lowercase ASCII-alphanumeric
   runs, length ≥ 2, fixed English stopword list. Pure function, no I/O.
3. **Provider seam** (`provider.rs`): `ScoreProvider` (query tokens +
   document fields → score), default `LocalLexicalProvider` (field weights:
   name 3.0, capability 2.5, description 1.0, text 0.5). `EmbeddingProvider`
   is the future seam; the only shipped implementation
   (`DisabledEmbeddingProvider`) fails closed so no external API can be
   reached by accident.
4. **Baseline ranking** (`ranking.rs`): filters first (disabled tools never
   rank; capability allow-list; risk ceiling `low<medium<high<irreversible`;
   evidence requirement = manifest flag or a successful receipt), then
   scores: keyword overlap + capability bonus + success boost (capped) +
   evidence boost − failure/denial penalty (capped, floor 0). Ties break by
   tool id. Every candidate explains itself (`reason`, `matchedFields`,
   `riskLevel`, `requiredApprovals` mirroring the policy gates).
5. **Read-only runtime**: `query_tool_candidates` rebuilds the index from
   the registry + ledger (manifests, `tool_receipt` outcomes, task titles,
   evidence kinds, compensation strategies) and ranks with the lexical
   provider. It writes index tables but never judges, persists, or emits —
   receipts keep flowing through the existing `invoke` path.
6. **UI**: Tool-RAG panel on the Tools page (intent box, capability/risk
   filters, evidence toggle, top-K, candidate list with score/reason/risk
   badge/approvals, click-through to the manifest).

## Consequences

- Ranking is boring on purpose: lexical, capped, deterministic. A real
  embedding provider can replace the scorer behind `ScoreProvider` without
  changing the query model, the IPC shape, or any invariant.
- The query path needs no human approval surface: it is read-only and never
  executes tools. Execution still goes through `invoke_tool*` + policy.
- Receipt statistics aggregate from the ledger on the fly (`stats_from_evidence`);
  `record_receipt`/`fetch_receipt_stats` remain as the incremental API for a
  future live-updating index.
