//! Budget accounting: per-layer allocation, overflow detection, and
//! priority-ordered rebalancing targets.
//!
//! Protected layers (safety/system/task/session) are never rebalanced away.
//! When the snapshot overflows, [`rebalance_targets`] keeps protected layers
//! whole and hands the deficit to discardable layers from the lowest priority
//! upward (scratchpad → tool → evidence). Evidence keeps its references: it is
//! trimmed, never dropped while it carries refs (enforced in `compact`).

use std::collections::HashMap;

use crate::estimator::{estimate_tokens, snapshot_tokens};
use crate::model::{
    BudgetConfig, BudgetReport, CompressionPolicy, ContextLayer, ContextSnapshot, DiscardPolicy,
    LayerConfig, LayerUsage,
};

/// Canonical per-layer defaults: 8_000 tokens total, protected head layers.
#[must_use]
pub fn layer_defaults() -> Vec<LayerConfig> {
    vec![
        LayerConfig {
            layer: ContextLayer::Safety,
            priority: ContextLayer::Safety.priority(),
            max_budget: 800,
            compression: CompressionPolicy::Verbatim,
            discard: DiscardPolicy::Never,
            protected: true,
        },
        LayerConfig {
            layer: ContextLayer::System,
            priority: ContextLayer::System.priority(),
            max_budget: 800,
            compression: CompressionPolicy::Verbatim,
            discard: DiscardPolicy::Never,
            protected: true,
        },
        LayerConfig {
            layer: ContextLayer::Task,
            priority: ContextLayer::Task.priority(),
            max_budget: 1600,
            compression: CompressionPolicy::Verbatim,
            discard: DiscardPolicy::Never,
            protected: true,
        },
        LayerConfig {
            layer: ContextLayer::Session,
            priority: ContextLayer::Session.priority(),
            max_budget: 1200,
            compression: CompressionPolicy::Verbatim,
            discard: DiscardPolicy::Never,
            protected: true,
        },
        LayerConfig {
            layer: ContextLayer::Evidence,
            priority: ContextLayer::Evidence.priority(),
            max_budget: 1600,
            compression: CompressionPolicy::DedupeLines,
            discard: DiscardPolicy::Never,
            protected: false,
        },
        LayerConfig {
            layer: ContextLayer::Tool,
            priority: ContextLayer::Tool.priority(),
            max_budget: 1200,
            compression: CompressionPolicy::TrimTail,
            discard: DiscardPolicy::DropWhenOverBudget,
            protected: false,
        },
        LayerConfig {
            layer: ContextLayer::Scratchpad,
            priority: ContextLayer::Scratchpad.priority(),
            max_budget: 800,
            compression: CompressionPolicy::TrimTail,
            discard: DiscardPolicy::DropWhenOverBudget,
            protected: false,
        },
    ]
}

/// Canonical budget: 8_000 tokens total, [`layer_defaults`] per layer.
#[must_use]
pub fn default_config() -> BudgetConfig {
    BudgetConfig {
        total_budget: 8_000,
        layers: layer_defaults(),
    }
}

/// Maximum tokens configured for one layer (0 when unlisted).
#[must_use]
pub fn layer_max(config: &BudgetConfig, layer: ContextLayer) -> u64 {
    config
        .layers
        .iter()
        .find(|row| row.layer == layer)
        .map(|row| row.max_budget)
        .unwrap_or(0)
}

/// Whether one layer is protected (defaults to true when unlisted).
#[must_use]
pub fn layer_protected(config: &BudgetConfig, layer: ContextLayer) -> bool {
    config
        .layers
        .iter()
        .find(|row| row.layer == layer)
        .map(|row| row.protected)
        .unwrap_or(true)
}

/// Account one snapshot against a budget: per-layer usage + overflow flags.
#[must_use]
pub fn report(snapshot: &ContextSnapshot, config: &BudgetConfig) -> BudgetReport {
    let mut layers = Vec::with_capacity(ContextLayer::ordered().len());
    for layer in ContextLayer::ordered() {
        let used: u64 = snapshot
            .segments
            .iter()
            .filter(|segment| segment.layer == layer)
            .map(|segment| estimate_tokens(&segment.body))
            .sum();
        let max = layer_max(config, layer);
        let count = snapshot
            .segments
            .iter()
            .filter(|segment| segment.layer == layer)
            .count();
        layers.push(LayerUsage {
            layer,
            used_tokens: used,
            max_tokens: max,
            overflow: used > max,
            protected: layer_protected(config, layer),
            segment_count: count,
        });
    }
    let total_used = snapshot_tokens(snapshot);
    let layer_overflow = layers.iter().any(|usage| usage.overflow);
    BudgetReport {
        task_id: snapshot.task_id.clone(),
        layers,
        total_used,
        total_budget: config.total_budget,
        overflow: total_used > config.total_budget || layer_overflow,
    }
}

/// Rebalance targets: allowed tokens per layer after priority-ordered cuts.
///
/// Protected layers always keep their full usage. The deficit (total overflow
/// plus every discardable layer's own overflow) is reclaimed from the lowest
/// priority upward by shrinking allowed targets; [`compact`](crate::compact)
/// turns those targets into trims and reference-free drops.
#[must_use]
pub fn rebalance_targets(
    snapshot: &ContextSnapshot,
    config: &BudgetConfig,
) -> HashMap<ContextLayer, u64> {
    let current: HashMap<ContextLayer, u64> = ContextLayer::ordered()
        .into_iter()
        .map(|layer| {
            let used: u64 = snapshot
                .segments
                .iter()
                .filter(|segment| segment.layer == layer)
                .map(|segment| estimate_tokens(&segment.body))
                .sum();
            (layer, used)
        })
        .collect();
    let total: u64 = current.values().sum();
    let mut targets: HashMap<ContextLayer, u64> = current.clone();
    if total <= config.total_budget {
        // No total overflow: still clamp discardable layers to their maxima
        // so per-layer overflow is handled locally.
        for row in &config.layers {
            if !row.protected {
                if let Some(target) = targets.get_mut(&row.layer) {
                    *target = (*target).min(row.max_budget);
                }
            }
        }
        return targets;
    }
    let mut deficit = total.saturating_sub(config.total_budget);
    // Lowest priority first: scratchpad, tool, evidence.
    let mut reclaimable: Vec<&LayerConfig> =
        config.layers.iter().filter(|row| !row.protected).collect();
    reclaimable.sort_by_key(|row| std::cmp::Reverse(row.priority));
    for row in reclaimable {
        if deficit == 0 {
            break;
        }
        let used = current.get(&row.layer).copied().unwrap_or(0);
        let keep = used.min(row.max_budget);
        let reclaim = used.saturating_sub(keep).min(deficit);
        targets.insert(row.layer, used.saturating_sub(reclaim));
        deficit = deficit.saturating_sub(reclaim);
    }
    // Any remaining deficit is absorbed proportionally by the same
    // discardable layers (protected layers are never touched).
    if deficit > 0 {
        for row in config
            .layers
            .iter()
            .filter(|row| !row.protected)
            .collect::<Vec<_>>()
        {
            if deficit == 0 {
                break;
            }
            let allowed = targets.get(&row.layer).copied().unwrap_or(0);
            let cut = allowed.min(deficit);
            targets.insert(row.layer, allowed.saturating_sub(cut));
            deficit = deficit.saturating_sub(cut);
        }
    }
    targets
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ContextSegment, ContextSnapshot};

    fn segment(id: &str, layer: ContextLayer, chars: usize) -> ContextSegment {
        ContextSegment {
            id: id.to_owned(),
            layer,
            body: "x".repeat(chars),
            protected: false,
            safety_critical: false,
            schema_type: None,
            evidence_refs: vec![],
            compensation_refs: vec![],
            policy_refs: vec![],
            action_constraints: vec![],
        }
    }

    #[test]
    fn defaults_sum_to_total_and_protect_head_layers() {
        let config = default_config();
        assert_eq!(config.total_budget, 8_000);
        let sum: u64 = config.layers.iter().map(|row| row.max_budget).sum();
        assert_eq!(sum, 8_000);
        for layer in [
            ContextLayer::Safety,
            ContextLayer::System,
            ContextLayer::Task,
            ContextLayer::Session,
        ] {
            assert!(layer_protected(&config, layer), "{layer:?} protected");
        }
        assert!(!layer_protected(&config, ContextLayer::Scratchpad));
    }

    #[test]
    fn overflow_detected_per_layer_and_total() {
        let config = BudgetConfig {
            total_budget: 10,
            layers: layer_defaults(),
        };
        let snapshot = ContextSnapshot {
            task_id: "task-1".to_owned(),
            session_id: "session-local".to_owned(),
            segments: vec![
                segment("scratch-1", ContextLayer::Scratchpad, 4_001 * 4),
                segment("safe-1", ContextLayer::Safety, 4),
            ],
        };
        let report = report(&snapshot, &config);
        assert!(report.overflow);
        assert!(report.total_used > config.total_budget);
        let scratch = report
            .layers
            .iter()
            .find(|usage| usage.layer == ContextLayer::Scratchpad)
            .expect("scratch usage");
        assert!(scratch.overflow);
    }

    #[test]
    fn rebalancing_never_touches_protected_layers() {
        let config = default_config();
        let snapshot = ContextSnapshot {
            task_id: "task-1".to_owned(),
            session_id: "session-local".to_owned(),
            segments: vec![
                segment("safe-1", ContextLayer::Safety, 800 * 4),
                segment("scratch-1", ContextLayer::Scratchpad, 4_000 * 4),
            ],
        };
        let targets = rebalance_targets(&snapshot, &config);
        assert_eq!(
            targets.get(&ContextLayer::Safety).copied().unwrap_or(0),
            800
        );
        assert!(
            targets.get(&ContextLayer::Scratchpad).copied().unwrap_or(0) < 4_000,
            "scratchpad absorbs the deficit"
        );
    }
}
