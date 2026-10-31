//! Deterministic context budgeting and TypeCompact compaction foundation.
//!
//! Talos must feed large agent workflows to LLMs or local models without
//! losing safety constraints, task intent, evidence references, compensation
//! plans, or typed structure. TypeCompact preserves meaning and safety — it
//! never merely truncates text.
//!
//! Layers (priority 0 = kept first): safety, system, task, session, evidence,
//! tool, scratchpad. [`model::BudgetConfig::default`] fixes the total budget
//! (8_000 tokens) and per-layer maxima. [`estimator::estimate_tokens`] is a
//! local char-fallback estimator (no external calls, fully deterministic).
//! [`budget::report`] detects overflow; [`compact::compact_snapshot`] rebalances
//! from the lowest priority upward, compressing repetitive text and trimming
//! tails but never dropping protected or safety-critical segments.
//! [`invariants::check_compaction`] rejects any result that loses IDs, safety
//! constraints, evidence/compensation/policy references, or schema type names
//! with a structured [`model::CompactionError`].

pub mod budget;
pub mod compact;
pub mod estimator;
pub mod invariants;
pub mod model;
pub mod snapshot;

pub use budget::{default_config, layer_defaults, rebalance_targets, report};
pub use compact::{compact_snapshot, compress_body};
pub use estimator::{estimate_tokens, segment_tokens, snapshot_tokens};
pub use invariants::{canonical_payload, check_compaction, proof_hash};
pub use model::{
    BudgetConfig, BudgetReport, CompactedSegment, CompactionError, CompactionResult,
    CompressionPolicy, ContextLayer, ContextSegment, ContextSnapshot, DiscardPolicy, LayerConfig,
    LayerUsage, COMPACTION_KIND, COMPACTION_VERSION,
};
pub use snapshot::{
    build_diagnostic_snapshot, build_task_snapshot, safety_body, session_body, system_body,
    task_body,
};
