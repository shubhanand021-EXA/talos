//! Context Assembly Engine: budgeted, policy-safe prompt construction.
//!
//! [`assembly::assemble`] turns a [`gather::GatheredContext`] into an
//! [`assembly::AssembledContext`] (messages + tool schemas + audit metadata)
//! using the budget allocation in [`config`], token estimates from
//! [`estimator`], per-layer TypeCompact, and deterministic [`template`]
//! rendering. Safety content is verbatim and is never compacted.

pub mod assembly;
pub mod config;
pub mod estimator;
pub mod gather;
pub mod template;

pub use assembly::{
    assemble, assembly_evidence_payload, AssembledContext, AssemblyError, AssemblyInput,
    CompactionSummary,
};
pub use config::{AssemblyAllocation, ContextAssemblyConfig};
pub use estimator::{char_estimate, estimate_tokens, estimate_tokens_fallback, EstimatorKind};
pub use gather::{
    chain_head_hash, gather_evidence, gather_memory, gather_session, gather_task_contract,
    rank_tools, GatheredContext, HistoryTurn, RankedTool, SegmentInput, TaskContract, ToolEntry,
};
pub use template::TEMPLATE_VERSION;
