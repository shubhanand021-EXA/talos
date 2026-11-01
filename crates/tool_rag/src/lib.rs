//! Local-first Tool-RAG: deterministic retrieval over tool manifests,
//! receipts, evidence records, and task history.
//!
//! Talos must route intent to the correct tool before external embeddings
//! exist. This crate is the local foundation: a SQLite-backed index with
//! normalized tables and deterministic tokenization (no FTS5, no network),
//! a [`provider::ScoreProvider`] seam defaulting to
//! [`provider::LocalLexicalProvider`], and baseline ranking in
//! [`ranking::rank`] (keyword + capability match, receipt-history boosts,
//! failure penalties, risk-ceiling filtering).
//!
//! [`provider::EmbeddingProvider`] is the future seam; the only shipped
//! implementation ([`provider::DisabledEmbeddingProvider`]) fails closed.

pub mod index;
pub mod model;
pub mod provider;
pub mod ranking;
pub mod tokenizer;

pub use index::{
    build_indexed_tools, build_tool_text, ensure_rag_schema, evidence_kind_list,
    fetch_indexed_tools, fetch_receipt_stats, index_tools, plan_strategy_list, receipt_tool_tasks,
    record_receipt, stats_from_evidence,
};
pub use model::{
    required_approvals_for, risk_rank, IndexedTool, ReceiptStats, ToolCandidate, ToolQuery,
};
pub use provider::{
    DisabledEmbeddingProvider, DocTerms, EmbeddingProvider, LocalLexicalProvider, ScoreProvider,
    FIELD_WEIGHTS,
};
pub use ranking::{
    parse_retrieval_mode, rank, rank_with_semantic, semantic_text, validate_query, RetrievalMode,
    CAPABILITY_BONUS, EVIDENCE_BOOST, FAILURE_PENALTY_CAP, FAILURE_PENALTY_PER_RECEIPT,
    SUCCESS_BOOST_CAP, SUCCESS_BOOST_PER_RECEIPT,
};
pub use tokenizer::{split_tokens, token_counts, tokenize, STOPWORDS};
