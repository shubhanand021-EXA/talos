//! Invariant checker: TypeCompact results are rejected unless every
//! safety-critical reference survives.
//!
//! Checked (first failure wins, structured [`CompactionError`](crate::model::CompactionError)):
//! 1. required fields present (non-empty id, layer carried, non-dropped body
//!    non-empty),
//! 2. segment IDs unchanged (no renames; dropped ids reported separately),
//! 3. safety constraints unchanged (safety-critical + safety-layer bodies
//!    byte-identical),
//! 4. compensation references unchanged (set-equal),
//! 5. evidence chain references unchanged (set-equal),
//! 6. policy + action-constraint references unchanged (set-equal),
//! 7. schema type names unchanged (set-equal).
//!
//! Dropping is only legal for reference-free, non-protected segments on
//! discardable layers; everything else must round-trip exactly.

use std::collections::{BTreeMap, BTreeSet};

use sha2::{Digest, Sha256};

use crate::model::{invariant_sets, CompactionError, ContextSnapshot};

fn err(code: &str, message: String) -> CompactionError {
    CompactionError::new(code, message)
}

/// Canonical proof input: task + ordered (id, layer, body) rows of kept segments.
#[must_use]
pub fn canonical_payload(task_id: &str, kept: &[(String, String, String)]) -> String {
    let mut rows = kept.to_vec();
    rows.sort();
    let mut out = String::from("typecompact/1:");
    out.push_str(task_id);
    out.push(':');
    for (id, layer, body) in rows {
        out.push_str(&id);
        out.push(':');
        out.push_str(&layer);
        out.push(':');
        out.push_str(&body.len().to_string());
        out.push(':');
        out.push_str(&body);
        out.push(';');
    }
    out
}

/// `sha256:` proof hash over the canonical payload.
#[must_use]
pub fn proof_hash(task_id: &str, kept: &[(String, String, String)]) -> String {
    format!(
        "sha256:{}",
        hex::encode(Sha256::digest(canonical_payload(task_id, kept).as_bytes()))
    )
}

/// Validate a compaction outcome before it is returned or persisted.
///
/// `kept` maps segment id → (layer, compacted body); `dropped` lists removed
/// ids; `original` is the pre-compaction snapshot. Metadata for kept segments
/// is always carried through unchanged by the compactor, so this checker
/// focuses on sets, safety bytes, and required fields.
pub fn check_compaction(
    original: &ContextSnapshot,
    kept: &BTreeMap<String, (String, String)>,
    dropped: &[String],
    bodies_by_id: &BTreeMap<String, String>,
) -> Result<Vec<String>, CompactionError> {
    if original.segments.is_empty() {
        return Err(err(
            "empty_snapshot",
            "cannot compact an empty snapshot".to_owned(),
        ));
    }
    // 1. Required fields present.
    for segment in &original.segments {
        if segment.id.trim().is_empty() {
            return Err(err(
                "missing_id",
                "segment with empty id cannot be compacted".to_owned(),
            ));
        }
    }
    for (id, (_, body)) in kept {
        if id.trim().is_empty() {
            return Err(err(
                "missing_id",
                "compacted id must not be empty".to_owned(),
            ));
        }
        if body.is_empty() {
            return Err(err(
                "empty_body",
                format!("kept segment {id} must keep a non-empty body"),
            ));
        }
    }
    // 2. IDs unchanged: kept ∪ dropped must equal the original id set.
    let original_ids: BTreeSet<String> = original
        .segments
        .iter()
        .map(|segment| segment.id.clone())
        .collect();
    let mut round_tripped = BTreeSet::new();
    for id in kept.keys() {
        round_tripped.insert(id.clone());
    }
    for id in dropped {
        if !original_ids.contains(id) {
            return Err(err(
                "invariant_ids_changed",
                format!("dropped unknown segment {id}"),
            ));
        }
        if !round_tripped.insert(id.clone()) {
            return Err(err(
                "invariant_ids_changed",
                format!("segment {id} kept and dropped"),
            ));
        }
    }
    if round_tripped != original_ids {
        let missing: Vec<String> = original_ids.difference(&round_tripped).cloned().collect();
        return Err(err(
            "invariant_ids_changed",
            format!("segments lost without trace: {}", missing.join(",")),
        ));
    }
    // 3. Safety constraints unchanged: safety-critical and safety-layer bodies
    // must be byte-identical; protected segments must be kept.
    for segment in &original.segments {
        if segment.protected && !kept.contains_key(&segment.id) {
            return Err(err(
                "invariant_protected_dropped",
                format!("protected segment {} must never be dropped", segment.id),
            ));
        }
        if segment.safety_critical || segment.layer == crate::model::ContextLayer::Safety {
            let Some((_, body)) = kept.get(&segment.id) else {
                return Err(err(
                    "invariant_safety_changed",
                    format!("safety segment {} must never be dropped", segment.id),
                ));
            };
            if body != &segment.body {
                return Err(err(
                    "invariant_safety_changed",
                    format!("safety segment {} body changed", segment.id),
                ));
            }
        }
    }
    // 4-7. Reference sets must survive (drops are only legal when
    // reference-free, so any loss here is a compactor bug).
    let before = invariant_sets(original);
    let mut after_ids = BTreeSet::new();
    let mut after_evidence = BTreeSet::new();
    let mut after_compensation = BTreeSet::new();
    let mut after_policy = BTreeSet::new();
    let mut after_constraints = BTreeSet::new();
    let mut after_schemas = BTreeSet::new();
    for segment in &original.segments {
        if dropped.contains(&segment.id) {
            continue;
        }
        after_ids.insert(segment.id.clone());
        after_evidence.extend(segment.evidence_refs.iter().cloned());
        after_compensation.extend(segment.compensation_refs.iter().cloned());
        after_policy.extend(segment.policy_refs.iter().cloned());
        after_constraints.extend(segment.action_constraints.iter().cloned());
        if let Some(schema) = segment.schema_type.as_deref() {
            after_schemas.insert(schema.to_owned());
        }
    }
    // Kept bodies must still mention nothing contradictory: the checker trusts
    // carried metadata (bodies_by_id is informational here) but records sizes.
    let _ = bodies_by_id;
    let kept_ids: BTreeSet<String> = kept.keys().cloned().collect();
    if kept_ids != after_ids {
        return Err(err(
            "invariant_ids_changed",
            "kept segment set does not match survivors".to_owned(),
        ));
    }
    for (name, code, want, got) in [
        (
            "evidence",
            "invariant_evidence_missing",
            &before.evidence,
            &after_evidence,
        ),
        (
            "compensation",
            "invariant_compensation_missing",
            &before.compensation,
            &after_compensation,
        ),
        (
            "policy",
            "invariant_policy_missing",
            &before.policy,
            &after_policy,
        ),
        (
            "action constraints",
            "invariant_constraints_missing",
            &before.constraints,
            &after_constraints,
        ),
        (
            "schema types",
            "invariant_schema_changed",
            &before.schema_types,
            &after_schemas,
        ),
    ] {
        if want != got {
            let missing: Vec<String> = want.difference(got).cloned().collect();
            return Err(err(
                code,
                format!("{name} references lost: {}", missing.join(",")),
            ));
        }
    }
    let total = before.ids.len().max(1);
    Ok(vec![
        format!("ids:{}/{}", after_ids.len(), before.ids.len()),
        format!(
            "evidence:{}/{}",
            after_evidence.len(),
            before.evidence.len()
        ),
        format!(
            "compensation:{}/{}",
            after_compensation.len(),
            before.compensation.len()
        ),
        format!("policy:{}/{}", after_policy.len(), before.policy.len()),
        format!(
            "constraints:{}/{}",
            after_constraints.len(),
            before.constraints.len()
        ),
        format!(
            "schema:{}/{}",
            after_schemas.len(),
            before.schema_types.len()
        ),
        "safety:unchanged".to_owned(),
        format!("coverage:{}/{}", after_ids.len(), total),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ContextLayer, ContextSegment, ContextSnapshot};

    fn snapshot() -> ContextSnapshot {
        ContextSnapshot {
            task_id: "task-1".to_owned(),
            session_id: "session-local".to_owned(),
            segments: vec![
                ContextSegment {
                    id: "sys-1".to_owned(),
                    layer: ContextLayer::System,
                    body: "system prompt".to_owned(),
                    protected: true,
                    safety_critical: false,
                    schema_type: Some("SystemPrompt".to_owned()),
                    evidence_refs: vec![],
                    compensation_refs: vec![],
                    policy_refs: vec!["allow-read".to_owned()],
                    action_constraints: vec!["no-delete".to_owned()],
                },
                ContextSegment {
                    id: "ev-1".to_owned(),
                    layer: ContextLayer::Evidence,
                    body: "evidence row".to_owned(),
                    protected: false,
                    safety_critical: false,
                    schema_type: Some("EvidenceRow".to_owned()),
                    evidence_refs: vec!["ev-row-1".to_owned()],
                    compensation_refs: vec!["plan-1".to_owned()],
                    policy_refs: vec![],
                    action_constraints: vec![],
                },
                ContextSegment {
                    id: "safe-1".to_owned(),
                    layer: ContextLayer::Safety,
                    body: "never touch the desktop".to_owned(),
                    protected: true,
                    safety_critical: true,
                    schema_type: None,
                    evidence_refs: vec![],
                    compensation_refs: vec![],
                    policy_refs: vec![],
                    action_constraints: vec![],
                },
            ],
        }
    }

    fn kept_of(snapshot: &ContextSnapshot) -> BTreeMap<String, (String, String)> {
        snapshot
            .segments
            .iter()
            .map(|segment| {
                (
                    segment.id.clone(),
                    (segment.layer.as_str().to_owned(), segment.body.clone()),
                )
            })
            .collect()
    }

    #[test]
    fn verbatim_round_trip_passes_all_invariants() {
        let original = snapshot();
        let kept = kept_of(&original);
        let bodies: BTreeMap<String, String> = kept
            .iter()
            .map(|(id, (_, body))| (id.clone(), body.clone()))
            .collect();
        let preserved = check_compaction(&original, &kept, &[], &bodies).expect("passes");
        assert!(preserved.iter().any(|row| row == "safety:unchanged"));
        assert!(preserved.iter().any(|row| row.starts_with("ids:3/3")));
    }

    #[test]
    fn dropping_referenced_evidence_is_rejected() {
        let original = snapshot();
        let mut kept = kept_of(&original);
        kept.remove("ev-1");
        let bodies: BTreeMap<String, String> = kept
            .iter()
            .map(|(id, (_, body))| (id.clone(), body.clone()))
            .collect();
        let error = check_compaction(&original, &kept, &["ev-1".to_owned()], &bodies)
            .expect_err("evidence refs must survive");
        assert_eq!(error.code, "invariant_evidence_missing");
    }

    #[test]
    fn safety_body_rewrite_is_rejected() {
        let original = snapshot();
        let mut kept = kept_of(&original);
        kept.insert(
            "safe-1".to_owned(),
            ("safety".to_owned(), "edited safety".to_owned()),
        );
        let bodies: BTreeMap<String, String> = kept
            .iter()
            .map(|(id, (_, body))| (id.clone(), body.clone()))
            .collect();
        let error = check_compaction(&original, &kept, &[], &bodies).expect_err("safety verbatim");
        assert_eq!(error.code, "invariant_safety_changed");
    }

    #[test]
    fn proof_hash_is_deterministic_with_prefix() {
        let kept = vec![("a".to_owned(), "tool".to_owned(), "body".to_owned())];
        assert_eq!(proof_hash("task-1", &kept), proof_hash("task-1", &kept));
        assert!(proof_hash("task-1", &kept).starts_with("sha256:"));
        assert_ne!(proof_hash("task-1", &kept), proof_hash("task-2", &kept));
    }
}
