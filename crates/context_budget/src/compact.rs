//! TypeCompact: deterministic, invariant-checked compaction.
//!
//! Rules:
//! - Protected, safety-critical, and all safety-layer segments are verbatim.
//! - Only reference-free segments on discardable layers may be dropped, lowest
//!   priority first, and only while over budget.
//! - Compressible segments dedupe repeated lines, then trim the tail with an
//!   explicit `[…trimmed {n} chars…]` marker (never silent).
//! - After compaction [`check_compaction`](crate::invariants::check_compaction)
//!   must pass; otherwise the whole compaction is rejected with a structured
//!   [`CompactionError`](crate::model::CompactionError).

use std::collections::{BTreeMap, HashMap};

use crate::budget::{layer_max, rebalance_targets};
use crate::estimator::estimate_tokens;
use crate::invariants::{check_compaction, proof_hash};
use crate::model::{
    BudgetConfig, CompactedSegment, CompactionError, CompactionResult, CompressionPolicy,
    ContextLayer, ContextSnapshot,
};

fn err(code: &str, message: String) -> CompactionError {
    CompactionError::new(code, message)
}

/// Collapse whitespace runs inside one line (deterministic, ASCII-safe).
fn collapse_line(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut pending_space = false;
    for ch in line.chars() {
        if ch.is_whitespace() {
            pending_space = !out.is_empty();
        } else {
            if pending_space {
                out.push(' ');
                pending_space = false;
            }
            out.push(ch);
        }
    }
    out
}

/// Dedupe consecutive duplicate lines: keep the first, append a repeat marker.
fn dedupe_lines(body: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut previous: Option<String> = None;
    let mut repeats = 0_usize;
    for raw in body.lines() {
        let line = collapse_line(raw.trim());
        if line.is_empty() {
            continue;
        }
        if previous.as_deref() == Some(line.as_str()) {
            repeats += 1;
            continue;
        }
        if repeats > 0 {
            out.push(format!("[repeated x{repeats}]"));
            repeats = 0;
        }
        previous = Some(line.clone());
        out.push(line);
    }
    if repeats > 0 {
        out.push(format!("[repeated x{repeats}]"));
    }
    out.join("\n")
}

/// Trim a body to a token allowance, keeping a deterministic prefix plus an
/// explicit marker. Returns the trimmed body and the trimmed char count.
fn trim_to_tokens(body: &str, allowed_tokens: u64) -> (String, usize) {
    if estimate_tokens(body) <= allowed_tokens {
        return (body.to_owned(), 0);
    }
    // Reserve marker overhead (~8 tokens) so the result fits the allowance.
    let allowed_chars = (allowed_tokens.saturating_sub(8)).saturating_mul(4);
    let keep_chars = allowed_chars.max(16) as usize;
    let prefix: String = body.chars().take(keep_chars).collect();
    let trimmed = body.chars().count().saturating_sub(prefix.chars().count());
    (format!("{prefix}\n[…trimmed {trimmed} chars…]"), trimmed)
}

/// Compress one compressible body: dedupe, then trim to the allowance.
#[must_use]
pub fn compress_body(body: &str, allowed_tokens: u64, policy: CompressionPolicy) -> (String, bool) {
    if policy == CompressionPolicy::Verbatim {
        return (body.to_owned(), false);
    }
    let deduped = dedupe_lines(body);
    let deduped_changed = deduped != body;
    if estimate_tokens(&deduped) <= allowed_tokens {
        return (deduped, deduped_changed);
    }
    let (trimmed, _) = trim_to_tokens(&deduped, allowed_tokens);
    (trimmed, true)
}

/// Whether a segment may ever be dropped (reference-free, discardable layer).
fn droppable(segment: &crate::model::ContextSegment, config: &BudgetConfig) -> bool {
    if segment.protected || segment.safety_critical {
        return false;
    }
    if segment.layer == ContextLayer::Safety {
        return false;
    }
    if !segment.evidence_refs.is_empty()
        || !segment.compensation_refs.is_empty()
        || !segment.policy_refs.is_empty()
        || !segment.action_constraints.is_empty()
        || segment.schema_type.is_some()
    {
        return false;
    }
    config
        .layers
        .iter()
        .find(|row| row.layer == segment.layer)
        .is_some_and(|row| row.discard == crate::model::DiscardPolicy::DropWhenOverBudget)
}

/// Compact a snapshot against a budget (deterministic, invariant-checked).
pub fn compact_snapshot(
    snapshot: &ContextSnapshot,
    config: &BudgetConfig,
) -> Result<CompactionResult, CompactionError> {
    if snapshot.segments.is_empty() {
        return Err(err(
            "empty_snapshot",
            "cannot compact an empty snapshot".to_owned(),
        ));
    }
    if snapshot.task_id.trim().is_empty() {
        return Err(err(
            "missing_task",
            "snapshot task_id must be 1..=128 chars".to_owned(),
        ));
    }
    let original_tokens: u64 = snapshot
        .segments
        .iter()
        .map(|segment| estimate_tokens(&segment.body))
        .sum();

    // Fast path: within every limit — verbatim copy with proof hash.
    let total_over = original_tokens > config.total_budget;
    let layer_over = snapshot.segments.iter().any(|segment| {
        estimate_tokens(&segment.body) > layer_max(config, segment.layer)
            && !segment.protected
            && segment.layer != ContextLayer::Safety
            && !segment.safety_critical
    });
    // Per-layer usage also forces compaction when a discardable layer exceeds
    // its own maximum in aggregate.
    let mut per_layer: HashMap<ContextLayer, u64> = HashMap::new();
    for segment in &snapshot.segments {
        *per_layer.entry(segment.layer).or_insert(0) += estimate_tokens(&segment.body);
    }
    let aggregate_over = per_layer
        .iter()
        .any(|(layer, used)| *used > layer_max(config, *layer));

    if !total_over && !layer_over && !aggregate_over {
        let kept: BTreeMap<String, (String, String)> = snapshot
            .segments
            .iter()
            .map(|segment| {
                (
                    segment.id.clone(),
                    (segment.layer.as_str().to_owned(), segment.body.clone()),
                )
            })
            .collect();
        let bodies: BTreeMap<String, String> = kept
            .iter()
            .map(|(id, (_, body))| (id.clone(), body.clone()))
            .collect();
        let preserved = check_compaction(snapshot, &kept, &[], &bodies)?;
        let rows: Vec<(String, String, String)> = kept
            .iter()
            .map(|(id, (layer, body))| (id.clone(), layer.clone(), body.clone()))
            .collect();
        let proof = proof_hash(&snapshot.task_id, &rows);
        let short = proof
            .trim_start_matches("sha256:")
            .chars()
            .take(12)
            .collect::<String>();
        return Ok(CompactionResult {
            id: format!("compact-{}-{short}", snapshot.task_id),
            task_id: snapshot.task_id.clone(),
            original_tokens,
            compacted_tokens: original_tokens,
            preserved_invariants: preserved,
            dropped_segments: vec![],
            trimmed_segments: vec![],
            proof_hash: proof,
            segments: snapshot
                .segments
                .iter()
                .map(|segment| {
                    let tokens = estimate_tokens(&segment.body);
                    CompactedSegment {
                        id: segment.id.clone(),
                        layer: segment.layer,
                        body: segment.body.clone(),
                        original_tokens: tokens,
                        compacted_tokens: tokens,
                        trimmed: false,
                        dropped: false,
                        protected: segment.protected,
                    }
                })
                .collect(),
            version: crate::model::COMPACTION_VERSION.to_owned(),
        });
    }

    // Slow path: rebalance targets, then drop reference-free segments on
    // discardable layers (lowest priority first) while over budget, then trim
    // compressible survivors to their targets.
    let targets = rebalance_targets(snapshot, config);
    let mut dropped: Vec<String> = Vec::new();
    let mut kept_ids: Vec<String> = snapshot
        .segments
        .iter()
        .map(|segment| segment.id.clone())
        .collect();

    let mut usage: u64 = original_tokens;
    let mut candidates: Vec<&crate::model::ContextSegment> = snapshot
        .segments
        .iter()
        .filter(|segment| droppable(segment, config))
        .collect();
    candidates.sort_by_key(|segment| std::cmp::Reverse(segment.layer.priority()));
    for candidate in candidates {
        if usage <= config.total_budget {
            break;
        }
        usage = usage.saturating_sub(estimate_tokens(&candidate.body));
        dropped.push(candidate.id.clone());
        kept_ids.retain(|id| id != &candidate.id);
    }

    let mut segments: Vec<CompactedSegment> = Vec::with_capacity(snapshot.segments.len());
    let mut trimmed_ids: Vec<String> = Vec::new();
    for segment in &snapshot.segments {
        let original = estimate_tokens(&segment.body);
        if dropped.contains(&segment.id) {
            segments.push(CompactedSegment {
                id: segment.id.clone(),
                layer: segment.layer,
                body: String::new(),
                original_tokens: original,
                compacted_tokens: 0,
                trimmed: false,
                dropped: true,
                protected: segment.protected,
            });
            continue;
        }
        let verbatim =
            segment.protected || segment.safety_critical || segment.layer == ContextLayer::Safety;
        if verbatim {
            segments.push(CompactedSegment {
                id: segment.id.clone(),
                layer: segment.layer,
                body: segment.body.clone(),
                original_tokens: original,
                compacted_tokens: original,
                trimmed: false,
                dropped: false,
                protected: segment.protected,
            });
            continue;
        }
        let policy = config
            .layers
            .iter()
            .find(|row| row.layer == segment.layer)
            .map(|row| row.compression)
            .unwrap_or(CompressionPolicy::Verbatim);
        // Fair share of the layer target across its surviving segments.
        let survivors = snapshot
            .segments
            .iter()
            .filter(|other| {
                other.layer == segment.layer && !dropped.contains(&other.id) && {
                    let v = other.protected
                        || other.safety_critical
                        || other.layer == ContextLayer::Safety;
                    !v
                }
            })
            .count()
            .max(1) as u64;
        let layer_target = targets.get(&segment.layer).copied().unwrap_or(original);
        let allowed = layer_target.div_ceil(survivors);
        let (body, trimmed) = compress_body(&segment.body, allowed, policy);
        if trimmed {
            trimmed_ids.push(segment.id.clone());
        }
        let compacted = estimate_tokens(&body);
        segments.push(CompactedSegment {
            id: segment.id.clone(),
            layer: segment.layer,
            body,
            original_tokens: original,
            compacted_tokens: compacted,
            trimmed,
            dropped: false,
            protected: segment.protected,
        });
    }

    // Invariant gate: any loss rejects the whole compaction.
    let kept: BTreeMap<String, (String, String)> = segments
        .iter()
        .filter(|row| !row.dropped)
        .map(|row| {
            (
                row.id.clone(),
                (row.layer.as_str().to_owned(), row.body.clone()),
            )
        })
        .collect();
    let bodies: BTreeMap<String, String> = kept
        .iter()
        .map(|(id, (_, body))| (id.clone(), body.clone()))
        .collect();
    let preserved = check_compaction(snapshot, &kept, &dropped, &bodies)?;

    let compacted_tokens: u64 = segments.iter().map(|row| row.compacted_tokens).sum();
    let rows: Vec<(String, String, String)> = kept
        .iter()
        .map(|(id, (layer, body))| (id.clone(), layer.clone(), body.clone()))
        .collect();
    let proof = proof_hash(&snapshot.task_id, &rows);
    let short = proof
        .trim_start_matches("sha256:")
        .chars()
        .take(12)
        .collect::<String>();
    Ok(CompactionResult {
        id: format!("compact-{}-{short}", snapshot.task_id),
        task_id: snapshot.task_id.clone(),
        original_tokens,
        compacted_tokens,
        preserved_invariants: preserved,
        dropped_segments: dropped,
        trimmed_segments: trimmed_ids,
        proof_hash: proof,
        segments,
        version: crate::model::COMPACTION_VERSION.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budget::default_config;
    use crate::model::{ContextSegment, ContextSnapshot};

    fn seg(
        id: &str,
        layer: ContextLayer,
        body: &str,
        protected: bool,
        safety: bool,
    ) -> ContextSegment {
        ContextSegment {
            id: id.to_owned(),
            layer,
            body: body.to_owned(),
            protected,
            safety_critical: safety,
            schema_type: None,
            evidence_refs: vec![],
            compensation_refs: vec![],
            policy_refs: vec![],
            action_constraints: vec![],
        }
    }

    fn small_snapshot() -> ContextSnapshot {
        ContextSnapshot {
            task_id: "task-1".to_owned(),
            session_id: "session-local".to_owned(),
            segments: vec![
                seg("sys-1", ContextLayer::System, "system prompt", true, false),
                seg(
                    "safe-1",
                    ContextLayer::Safety,
                    "never bypass policy",
                    true,
                    true,
                ),
                seg("note-1", ContextLayer::Scratchpad, "note", false, false),
            ],
        }
    }

    #[test]
    fn within_budget_returns_verbatim_copy() {
        let result = compact_snapshot(&small_snapshot(), &default_config()).expect("compacts");
        assert_eq!(result.original_tokens, result.compacted_tokens);
        assert!(result.dropped_segments.is_empty());
        assert!(result.trimmed_segments.is_empty());
        assert!(result.proof_hash.starts_with("sha256:"));
        assert!(result
            .preserved_invariants
            .iter()
            .any(|row| row == "safety:unchanged"));
    }

    #[test]
    fn compaction_reduces_repetitive_scratchpad() {
        let body = (0..200)
            .map(|_| "same line here")
            .collect::<Vec<_>>()
            .join("\n");
        let snapshot = ContextSnapshot {
            task_id: "task-1".to_owned(),
            session_id: "session-local".to_owned(),
            segments: vec![
                seg("sys-1", ContextLayer::System, "system prompt", true, false),
                seg(
                    "safe-1",
                    ContextLayer::Safety,
                    "never bypass policy",
                    true,
                    true,
                ),
                seg("scratch-1", ContextLayer::Scratchpad, &body, false, false),
            ],
        };
        let config = crate::model::BudgetConfig {
            total_budget: 100,
            layers: crate::budget::layer_defaults(),
        };
        let result = compact_snapshot(&snapshot, &config).expect("compacts");
        assert!(result.compacted_tokens < result.original_tokens);
        assert!(!result.trimmed_segments.is_empty() || !result.dropped_segments.is_empty());
        // Protected bodies survive byte-identical.
        let safe = result
            .segments
            .iter()
            .find(|row| row.id == "safe-1")
            .expect("safe");
        assert!(!safe.trimmed && !safe.dropped);
        assert_eq!(safe.body, "never bypass policy");
    }

    #[test]
    fn protected_layer_never_dropped_under_pressure() {
        let snapshot = ContextSnapshot {
            task_id: "task-1".to_owned(),
            session_id: "session-local".to_owned(),
            segments: vec![
                seg(
                    "sys-1",
                    ContextLayer::System,
                    &"s".repeat(4_000),
                    true,
                    false,
                ),
                seg(
                    "safe-1",
                    ContextLayer::Safety,
                    &"p".repeat(4_000),
                    true,
                    true,
                ),
                seg(
                    "scratch-1",
                    ContextLayer::Scratchpad,
                    &"n".repeat(4_000),
                    false,
                    false,
                ),
                seg(
                    "tool-1",
                    ContextLayer::Tool,
                    &"t".repeat(4_000),
                    false,
                    false,
                ),
            ],
        };
        let config = crate::model::BudgetConfig {
            total_budget: 100,
            layers: crate::budget::layer_defaults(),
        };
        let result = compact_snapshot(&snapshot, &config).expect("compacts");
        assert!(!result.dropped_segments.contains(&"sys-1".to_owned()));
        assert!(!result.dropped_segments.contains(&"safe-1".to_owned()));
    }

    #[test]
    fn compensation_and_evidence_refs_survive() {
        let snapshot = ContextSnapshot {
            task_id: "task-1".to_owned(),
            session_id: "session-local".to_owned(),
            segments: vec![
                seg("sys-1", ContextLayer::System, "system prompt", true, false),
                ContextSegment {
                    id: "ev-1".to_owned(),
                    layer: ContextLayer::Evidence,
                    body: "evidence with a very long repetitive tail ".repeat(50),
                    protected: false,
                    safety_critical: false,
                    schema_type: Some("EvidenceRow".to_owned()),
                    evidence_refs: vec!["ev-row-9".to_owned()],
                    compensation_refs: vec!["plan-9".to_owned()],
                    policy_refs: vec!["allow-x".to_owned()],
                    action_constraints: vec!["no-delete".to_owned()],
                },
                seg(
                    "scratch-1",
                    ContextLayer::Scratchpad,
                    &"n".repeat(8_000),
                    false,
                    false,
                ),
            ],
        };
        let config = crate::model::BudgetConfig {
            total_budget: 200,
            layers: crate::budget::layer_defaults(),
        };
        let result = compact_snapshot(&snapshot, &config).expect("compacts");
        assert!(!result.dropped_segments.contains(&"ev-1".to_owned()));
        assert!(result
            .preserved_invariants
            .iter()
            .any(|row| row.starts_with("evidence:1/1")));
        assert!(result
            .preserved_invariants
            .iter()
            .any(|row| row.starts_with("compensation:1/1")));
    }

    #[test]
    fn deterministic_output_for_same_input() {
        let snapshot = small_snapshot();
        let config = default_config();
        let first = compact_snapshot(&snapshot, &config).expect("first");
        let second = compact_snapshot(&snapshot, &config).expect("second");
        assert_eq!(first, second);
        assert_eq!(first.proof_hash, second.proof_hash);
    }

    #[test]
    fn compress_body_dedupes_repetition() {
        let body = "alpha\nalpha\nalpha\nbeta";
        let (out, trimmed) = compress_body(body, 1_000, CompressionPolicy::DedupeLines);
        assert!(trimmed);
        assert!(out.contains("[repeated x2]"));
        assert!(out.contains("beta"));
    }

    #[test]
    fn empty_snapshot_rejected() {
        let snapshot = ContextSnapshot {
            task_id: "task-1".to_owned(),
            session_id: "session-local".to_owned(),
            segments: vec![],
        };
        let error = compact_snapshot(&snapshot, &default_config()).expect_err("empty rejected");
        assert_eq!(error.code, "empty_snapshot");
    }
}
