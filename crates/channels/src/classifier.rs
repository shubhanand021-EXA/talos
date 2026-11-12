//! Fail-closed goal classifier: maps free text to a policy-gate shape.
//!
//! The broker remains the sole authority; this only shapes the
//! [`policy_broker::ActionRequest`]. Tiers, checked in order:
//!
//! 1. **Wildcard-destructive** (`delete_all`, `rm -rf`, `format`, `wipe`,
//!    `drop`, `*.*`): unscoped — evaluated at least privilege
//!    (`ReadFile`/`Medium`), which Rule 6 default-deny rejects. Nothing
//!    unscoped ever executes.
//! 2. **High-risk** (`delete`, `exec`, `deploy`, `publish`, `send`, `push`,
//!    `install`, `shutdown`, `remove <path>`, ...): `SystemControl`/`High`
//!    non-reversible → human review (`waiting_policy` + desktop notification).
//! 3. **Scoped mutation** (`write`, `create`, `update`, `run`, `commit`, ...):
//!    `ExecuteCommand`/`Medium` reversible → broker asks for evidence the
//!    channel cannot supply, so the gateway refuses with the broker's reason.
//! 4. **Read-only default** (`read`, `list`, `summarize`, `status`, ... and
//!    anything unrecognized): `ReadFile`/`Low` reversible → allow + execute.

use policy_broker::{Capability, RiskLevel};

/// Policy-gate shape for one goal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GateShape {
    /// Capability to exercise.
    pub capability: Capability,
    /// Declared risk.
    pub risk: RiskLevel,
    /// Whether the action is reversible.
    pub reversible: bool,
}

/// Wildcard-destructive phrases: no scoped tool call can express them.
const DESTRUCTIVE_WILDCARD: &[&str] = &[
    "delete_all",
    "delete all",
    "rm -rf",
    "rm -rf /",
    "format ",
    "format c:",
    "wipe",
    "drop table",
    "drop database",
    "*.*",
    "del /f /s /q",
];

/// High-risk verbs: human review, never auto-execute.
const HIGH_RISK_VERBS: &[&str] = &[
    "delete",
    "remove",
    "exec",
    "execute",
    "deploy",
    "publish",
    "send",
    "push",
    "install",
    "uninstall",
    "shutdown",
    "restart",
    "kill",
    "terminate",
    "grant",
    "revoke",
];

/// Scoped-mutation verbs: broker evidence-gated, refused from chat.
const SCOPED_MUTATION_VERBS: &[&str] = &[
    "write", "create", "update", "modify", "edit", "run", "commit", "test", "build", "fix",
    "refactor", "migrate",
];

/// Classify one goal into a gate shape. Deterministic, total, no I/O.
#[must_use]
pub fn classify_goal(goal: &str) -> GateShape {
    let lower = goal.to_ascii_lowercase();
    if DESTRUCTIVE_WILDCARD
        .iter()
        .any(|phrase| lower.contains(phrase))
    {
        // Fail closed: least-privilege shape the broker's default-deny
        // rejects (Rule 6 — no allow-rule covers it).
        return GateShape {
            capability: Capability::ReadFile,
            risk: RiskLevel::Medium,
            reversible: true,
        };
    }
    if HIGH_RISK_VERBS
        .iter()
        .any(|verb| contains_word(&lower, verb))
    {
        return GateShape {
            capability: Capability::SystemControl,
            risk: RiskLevel::High,
            reversible: false,
        };
    }
    if SCOPED_MUTATION_VERBS
        .iter()
        .any(|verb| contains_word(&lower, verb))
    {
        return GateShape {
            capability: Capability::ExecuteCommand,
            risk: RiskLevel::Medium,
            reversible: true,
        };
    }
    GateShape {
        capability: Capability::ReadFile,
        risk: RiskLevel::Low,
        reversible: true,
    }
}

/// Whole-word match (`delete` matches "delete the file", not "undelete").
fn contains_word(haystack: &str, needle: &str) -> bool {
    haystack
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|token| token == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_destructive_maps_to_default_deny_shape() {
        for goal in [
            "fs.delete_all",
            "please delete all files",
            "rm -rf / tmp",
            "format the drive",
            "wipe everything",
        ] {
            let shape = classify_goal(goal);
            assert_eq!(
                shape,
                GateShape {
                    capability: Capability::ReadFile,
                    risk: RiskLevel::Medium,
                    reversible: true,
                },
                "fail-closed shape for {goal}"
            );
        }
    }

    #[test]
    fn high_risk_verbs_map_to_human_review_shape() {
        let shape = classify_goal("delete the temp files");
        assert_eq!(shape.risk, RiskLevel::High);
        assert!(!shape.reversible);
        assert_eq!(shape.capability, Capability::SystemControl);
    }

    #[test]
    fn scoped_mutations_map_to_evidence_gated_shape() {
        let shape = classify_goal("run the test suite");
        assert_eq!(
            shape,
            GateShape {
                capability: Capability::ExecuteCommand,
                risk: RiskLevel::Medium,
                reversible: true,
            }
        );
    }

    #[test]
    fn reads_and_unknown_text_default_to_allow_shape() {
        assert_eq!(classify_goal("summarize the ledger").risk, RiskLevel::Low);
        assert_eq!(classify_goal("hello talos").risk, RiskLevel::Low);
        // Whole-word: "undelete" is not the delete verb.
        assert_eq!(classify_goal("undelete my draft").risk, RiskLevel::Low);
    }
}
