//! Layered context model: seven layers, each with priority, budget,
//! compression/discard policy, and a protected flag.
//!
//! Wire shapes mirror `@talos/contracts` `context.ts` exactly (`camelCase`
//! JSON) so the TypeScript bridge validates every response with Zod.

use std::collections::BTreeSet;

/// Stable evidence kind for persisted compaction receipts.
pub const COMPACTION_KIND: &str = "context_compaction";
/// TypeCompact engine version stamped on every receipt.
pub const COMPACTION_VERSION: &str = "typecompact/1";

/// One of the seven context layers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextLayer {
    /// Fixed product instructions (protected, verbatim).
    System,
    /// Durable session identity (protected, verbatim).
    Session,
    /// Owning task intent and lifecycle (protected, verbatim).
    Task,
    /// Ledger evidence references (evidence-gated, trimmed never dropped w/refs).
    Evidence,
    /// Tool outputs and receipts (droppable when reference-free).
    Tool,
    /// Working notes and scratch state (first to drop/trim).
    Scratchpad,
    /// Safety constraints and policy state (protected, always verbatim).
    Safety,
}

impl ContextLayer {
    /// All layers in priority order (index 0 = kept first).
    #[must_use]
    pub const fn ordered() -> [ContextLayer; 7] {
        [
            ContextLayer::Safety,
            ContextLayer::System,
            ContextLayer::Task,
            ContextLayer::Session,
            ContextLayer::Evidence,
            ContextLayer::Tool,
            ContextLayer::Scratchpad,
        ]
    }

    /// Keep order: lower number survives rebalancing longer.
    #[must_use]
    pub const fn priority(self) -> u8 {
        match self {
            ContextLayer::Safety => 0,
            ContextLayer::System => 1,
            ContextLayer::Task => 2,
            ContextLayer::Session => 3,
            ContextLayer::Evidence => 4,
            ContextLayer::Tool => 5,
            ContextLayer::Scratchpad => 6,
        }
    }

    /// Wire name (`snake_case`, stable).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ContextLayer::System => "system",
            ContextLayer::Session => "session",
            ContextLayer::Task => "task",
            ContextLayer::Evidence => "evidence",
            ContextLayer::Tool => "tool",
            ContextLayer::Scratchpad => "scratchpad",
            ContextLayer::Safety => "safety",
        }
    }
}

/// How a layer's bodies may be compressed (never applies to protected layers).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompressionPolicy {
    /// Byte-identical copy (safety/system/task/session).
    Verbatim,
    /// Dedupe repeated lines, then trim the tail with a marker.
    TrimTail,
    /// Dedupe repeated lines only; trim only when still over budget.
    DedupeLines,
}

/// When a layer may lose whole segments under overflow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscardPolicy {
    /// Segments are never dropped (protected layers + evidence/tool with refs).
    Never,
    /// Reference-free segments may be dropped, lowest priority first.
    DropWhenOverBudget,
}

/// Per-layer budget and policy.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerConfig {
    /// Layer this row governs.
    pub layer: ContextLayer,
    /// Keep order (0 = kept first).
    pub priority: u8,
    /// Maximum tokens for this layer.
    pub max_budget: u64,
    /// How bodies may be compressed.
    pub compression: CompressionPolicy,
    /// When whole segments may be dropped.
    pub discard: DiscardPolicy,
    /// True for safety/system/task/session: never trimmed, never dropped.
    pub protected: bool,
}

/// Total budget plus per-layer configuration.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetConfig {
    /// Maximum tokens for the whole snapshot.
    pub total_budget: u64,
    /// One row per layer (priority order).
    pub layers: Vec<LayerConfig>,
}

/// One typed context segment with explicit invariant references.
///
/// TypeCompact only ever rewrites `body`. Every other field is carried
/// through byte-identical, which is what makes preservation checkable.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextSegment {
    /// Stable segment identifier (preserved verbatim).
    pub id: String,
    /// Owning layer.
    pub layer: ContextLayer,
    /// Free-text payload (the only field compaction may rewrite).
    pub body: String,
    /// Explicitly protected: never trimmed, never dropped.
    pub protected: bool,
    /// Safety-critical: never trimmed, never dropped, body must match exactly.
    pub safety_critical: bool,
    /// Schema type name (e.g. `ToolReceipt`); the set must survive compaction.
    pub schema_type: Option<String>,
    /// Evidence row references (must all survive).
    pub evidence_refs: Vec<String>,
    /// Compensation plan references (must all survive).
    pub compensation_refs: Vec<String>,
    /// Policy decision references (must all survive).
    pub policy_refs: Vec<String>,
    /// Action constraint references (must all survive).
    pub action_constraints: Vec<String>,
}

/// One task's full layered context.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextSnapshot {
    /// Owning task.
    pub task_id: String,
    /// Owning session.
    pub session_id: String,
    /// All segments (any order; reports group by layer).
    pub segments: Vec<ContextSegment>,
}

/// Token usage for one layer.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerUsage {
    /// Layer measured.
    pub layer: ContextLayer,
    /// Tokens used by this layer.
    pub used_tokens: u64,
    /// Configured maximum.
    pub max_tokens: u64,
    /// True when `used_tokens > max_tokens`.
    pub overflow: bool,
    /// Protected layers are never rebalanced away.
    pub protected: bool,
    /// Segment count in this layer.
    pub segment_count: usize,
}

/// Budget accounting for a whole snapshot.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetReport {
    /// Owning task.
    pub task_id: String,
    /// Per-layer usage in priority order.
    pub layers: Vec<LayerUsage>,
    /// Sum over all layers.
    pub total_used: u64,
    /// Configured total maximum.
    pub total_budget: u64,
    /// True when `total_used > total_budget` or any layer overflows.
    pub overflow: bool,
}

/// One segment after TypeCompact.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactedSegment {
    /// Stable identifier (unchanged).
    pub id: String,
    /// Owning layer (unchanged).
    pub layer: ContextLayer,
    /// Compacted body (verbatim for protected/safety-critical segments).
    pub body: String,
    /// Tokens before compaction.
    pub original_tokens: u64,
    /// Tokens after compaction.
    pub compacted_tokens: u64,
    /// True when the body was rewritten.
    pub trimmed: bool,
    /// True when the segment was dropped (reference-free, discardable only).
    pub dropped: bool,
    /// Protected segments are always carried verbatim.
    pub protected: bool,
}

/// Deterministic TypeCompact outcome.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionResult {
    /// Stable receipt identifier (`compact-{task_id}-{proof short hash}`).
    pub id: String,
    /// Owning task.
    pub task_id: String,
    /// Tokens before compaction.
    pub original_tokens: u64,
    /// Tokens after compaction.
    pub compacted_tokens: u64,
    /// Human-readable invariant attestations (`ids:12/12`, ...).
    pub preserved_invariants: Vec<String>,
    /// Dropped segment ids (reference-free, discardable layers only).
    pub dropped_segments: Vec<String>,
    /// Trimmed segment ids.
    pub trimmed_segments: Vec<String>,
    /// `sha256:` hash of the canonical compacted payload.
    pub proof_hash: String,
    /// Per-segment outcomes in input order.
    pub segments: Vec<CompactedSegment>,
    /// Engine version (`typecompact/1`).
    pub version: String,
}

/// Structured TypeCompact failure (invariant rejected, never silent).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CompactionError {
    /// Stable code (`invariant_*`, `empty_snapshot`, ...).
    pub code: String,
    /// Human-readable detail (safe to display).
    pub message: String,
}

impl CompactionError {
    /// Build a structured failure.
    #[must_use]
    pub fn new(code: &str, message: String) -> Self {
        Self {
            code: code.to_owned(),
            message,
        }
    }
}

impl std::fmt::Display for CompactionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for CompactionError {}

/// Extract the sorted invariant reference sets of a snapshot.
#[must_use]
pub fn invariant_sets(snapshot: &ContextSnapshot) -> InvariantSets {
    let mut ids = BTreeSet::new();
    let mut evidence = BTreeSet::new();
    let mut compensation = BTreeSet::new();
    let mut policy = BTreeSet::new();
    let mut constraints = BTreeSet::new();
    let mut schema_types = BTreeSet::new();
    for segment in &snapshot.segments {
        ids.insert(segment.id.clone());
        evidence.extend(segment.evidence_refs.iter().cloned());
        compensation.extend(segment.compensation_refs.iter().cloned());
        policy.extend(segment.policy_refs.iter().cloned());
        constraints.extend(segment.action_constraints.iter().cloned());
        if let Some(schema) = segment.schema_type.as_deref() {
            schema_types.insert(schema.to_owned());
        }
    }
    InvariantSets {
        ids,
        evidence,
        compensation,
        policy,
        constraints,
        schema_types,
    }
}

/// Sorted invariant reference sets (deterministic, comparable).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InvariantSets {
    /// All segment ids.
    pub ids: BTreeSet<String>,
    /// All evidence references.
    pub evidence: BTreeSet<String>,
    /// All compensation references.
    pub compensation: BTreeSet<String>,
    /// All policy references.
    pub policy: BTreeSet<String>,
    /// All action-constraint references.
    pub constraints: BTreeSet<String>,
    /// All schema type names.
    pub schema_types: BTreeSet<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layers_cover_all_seven_in_priority_order() {
        let ordered = ContextLayer::ordered();
        assert_eq!(ordered.len(), 7);
        let mut priorities: Vec<u8> = ordered.iter().map(|layer| layer.priority()).collect();
        priorities.sort_unstable();
        assert_eq!(priorities, vec![0, 1, 2, 3, 4, 5, 6]);
        assert_eq!(ContextLayer::Safety.priority(), 0);
        assert_eq!(ContextLayer::Scratchpad.priority(), 6);
    }

    #[test]
    fn layer_names_are_stable() {
        assert_eq!(ContextLayer::Safety.as_str(), "safety");
        assert_eq!(ContextLayer::Scratchpad.as_str(), "scratchpad");
        let raw = serde_json::to_string(&ContextLayer::Scratchpad).expect("serializes");
        assert_eq!(raw, "\"scratchpad\"");
    }
}
