//! Deterministic snapshot builders: task state + ledger rows → layers.
//!
//! The desktop layer owns I/O (task lookup, ledger reads); this module owns
//! the pure mapping so builders stay testable and byte-deterministic.

use crate::model::{ContextLayer, ContextSegment, ContextSnapshot};

#[allow(clippy::too_many_arguments)]
fn segment(
    id: String,
    layer: ContextLayer,
    body: String,
    protected: bool,
    safety_critical: bool,
    schema_type: Option<&str>,
    evidence_refs: Vec<String>,
    compensation_refs: Vec<String>,
    policy_refs: Vec<String>,
    action_constraints: Vec<String>,
) -> ContextSegment {
    ContextSegment {
        id,
        layer,
        body,
        protected,
        safety_critical,
        schema_type: schema_type.map(str::to_owned),
        evidence_refs,
        compensation_refs,
        policy_refs,
        action_constraints,
    }
}

/// Fixed system instructions (protected, verbatim).
#[must_use]
pub fn system_body() -> String {
    "Talos context: policy-gated agent. Never bypass the policy broker. \
     Every side effect needs evidence; irreversible actions need compensation. \
     Dry-run before mutating."
        .to_owned()
}

/// Session identity row (protected, verbatim).
#[must_use]
pub fn session_body(session_id: &str) -> String {
    format!("session {session_id} (local, isolated profiles, no personal data)")
}

/// Task intent row (protected, verbatim).
#[must_use]
pub fn task_body(task_id: &str, title: &str, status: &str) -> String {
    format!("task {task_id}: {title} [status={status}]")
}

/// Safety constraints row (protected + safety-critical, always verbatim).
#[must_use]
pub fn safety_body(safety_summary: &str) -> String {
    format!("safety envelope: {safety_summary}. Emergency stop latches on violation.")
}

/// Build a full seven-layer snapshot from task state and ledger excerpts.
///
/// - `evidence`: `(row_id, kind, status)` rows → one segment each (refs kept).
/// - `tools`: `(tool_id, receipt_id, status)` rows → one segment each.
/// - `notes`: free scratch text (first trimmed/dropped under pressure).
/// - `policy_refs` / `constraints`: task-level policy + constraint refs.
/// - `compensation_refs`: plan ids covering this task.
#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn build_task_snapshot(
    task_id: &str,
    session_id: &str,
    title: &str,
    status: &str,
    safety_summary: &str,
    evidence: &[(String, String, String)],
    tools: &[(String, String, String)],
    notes: &str,
    policy_refs: Vec<String>,
    compensation_refs: Vec<String>,
    constraints: Vec<String>,
) -> ContextSnapshot {
    let mut segments = vec![
        segment(
            format!("system-{task_id}"),
            ContextLayer::System,
            system_body(),
            true,
            false,
            Some("SystemPrompt"),
            vec![],
            vec![],
            vec!["default-deny".to_owned()],
            vec!["no-bypass-policy".to_owned()],
        ),
        segment(
            format!("session-{session_id}"),
            ContextLayer::Session,
            session_body(session_id),
            true,
            false,
            Some("SessionIdentity"),
            vec![],
            vec![],
            vec![],
            vec![],
        ),
        segment(
            format!("task-{task_id}"),
            ContextLayer::Task,
            task_body(task_id, title, status),
            true,
            false,
            Some("TaskIntent"),
            vec![],
            compensation_refs.clone(),
            policy_refs.clone(),
            constraints.clone(),
        ),
        segment(
            format!("safety-{task_id}"),
            ContextLayer::Safety,
            safety_body(safety_summary),
            true,
            true,
            Some("SafetyEnvelope"),
            vec![],
            vec![],
            vec!["emergency-stop-latch".to_owned()],
            vec!["no-bypass-safety".to_owned()],
        ),
    ];
    for (row_id, kind, row_status) in evidence {
        segments.push(segment(
            format!("evidence-{row_id}"),
            ContextLayer::Evidence,
            format!("evidence {row_id} [{kind}] status={row_status}"),
            false,
            false,
            Some("EvidenceRow"),
            vec![row_id.clone()],
            vec![],
            vec![],
            vec![],
        ));
    }
    for (tool_id, receipt_id, receipt_status) in tools {
        segments.push(segment(
            format!("tool-{receipt_id}"),
            ContextLayer::Tool,
            format!("tool {tool_id} receipt {receipt_id} status={receipt_status}"),
            false,
            false,
            Some("ToolReceipt"),
            vec![receipt_id.clone()],
            vec![],
            vec![],
            vec![],
        ));
    }
    segments.push(segment(
        format!("scratch-{task_id}"),
        ContextLayer::Scratchpad,
        if notes.trim().is_empty() {
            format!("scratch notes for {task_id}: none")
        } else {
            format!("scratch notes for {task_id}: {notes}")
        },
        false,
        false,
        None,
        vec![],
        vec![],
        vec![],
        vec![],
    ));
    ContextSnapshot {
        task_id: task_id.to_owned(),
        session_id: session_id.to_owned(),
        segments,
    }
}

/// Build a diagnostic snapshot: diagnostic task + ledger counts + claim/plan refs.
#[must_use]
pub fn build_diagnostic_snapshot(
    task_id: &str,
    agent_id: &str,
    claim_id: &str,
    plan_id: &str,
    ledger_summary: &str,
) -> ContextSnapshot {
    build_task_snapshot(
        task_id,
        "session-local",
        "diagnostic loop",
        "completed",
        "enabled=false, dry-run default, stop clear",
        &[(
            format!("{task_id}-evidence"),
            "tool_receipt".to_owned(),
            "verified".to_owned(),
        )],
        &[(
            "diagnostic.probe".to_owned(),
            format!("{task_id}-receipt"),
            "success".to_owned(),
        )],
        ledger_summary,
        vec![format!("eval-{task_id}")],
        vec![plan_id.to_owned()],
        vec![format!("claim:{claim_id}"), format!("agent:{agent_id}")],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budget::default_config;
    use crate::compact::compact_snapshot;

    #[test]
    fn builder_covers_all_seven_layers() {
        let snapshot = build_task_snapshot(
            "task-1",
            "session-local",
            "demo",
            "executing",
            "stop clear",
            &[(
                "ev-1".to_owned(),
                "tool_receipt".to_owned(),
                "verified".to_owned(),
            )],
            &[(
                "fs.read".to_owned(),
                "receipt-1".to_owned(),
                "success".to_owned(),
            )],
            "working notes",
            vec!["allow-x".to_owned()],
            vec!["plan-1".to_owned()],
            vec!["no-delete".to_owned()],
        );
        let mut layers: Vec<ContextLayer> = snapshot
            .segments
            .iter()
            .map(|segment| segment.layer)
            .collect();
        layers.sort_by_key(|layer| layer.priority());
        layers.dedup();
        assert_eq!(layers.len(), 7);
        // Compacts cleanly within the default budget.
        let result = compact_snapshot(&snapshot, &default_config()).expect("compacts");
        assert_eq!(result.original_tokens, result.compacted_tokens);
        assert!(result.dropped_segments.is_empty());
    }

    #[test]
    fn builder_is_deterministic() {
        let first = build_task_snapshot(
            "task-1",
            "session-local",
            "demo",
            "executing",
            "stop clear",
            &[],
            &[],
            "notes",
            vec![],
            vec![],
            vec![],
        );
        let second = build_task_snapshot(
            "task-1",
            "session-local",
            "demo",
            "executing",
            "stop clear",
            &[],
            &[],
            "notes",
            vec![],
            vec![],
            vec![],
        );
        assert_eq!(first, second);
    }
}
