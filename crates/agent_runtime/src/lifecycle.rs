//! Strict task lifecycle state machine.
//!
//! States: `queued`, `planning`, `executing`, `waiting_policy`,
//! `waiting_evidence`, `waiting_compensation`, `canceling`, `canceled`,
//! `completed`, `failed`, `timed_out`. Terminal states are explicit and
//! transitions are validated before any ledger write.

/// All lifecycle states (canonical order, not execution order).
pub const STATES: &[&str] = &[
    "queued",
    "planning",
    "executing",
    "waiting_policy",
    "waiting_evidence",
    "waiting_compensation",
    "canceling",
    "canceled",
    "completed",
    "failed",
    "failed_needs_manual_review",
    "timed_out",
];

/// Terminal states: no outgoing transition except explicit `retry_task`
/// (`failed` / `failed_needs_manual_review` / `timed_out` / `canceled` -> `queued`).
#[must_use]
pub fn is_terminal(status: &str) -> bool {
    matches!(
        status,
        "completed" | "failed" | "failed_needs_manual_review" | "canceled" | "timed_out"
    )
}

/// True when `from` -> `to` is a legal lifecycle move.
#[must_use]
pub fn can_transition(from: &str, to: &str) -> bool {
    match (from, to) {
        ("queued", "planning" | "canceling") => true,
        ("planning", "executing" | "waiting_policy" | "failed" | "canceling") => true,
        (
            "executing",
            "waiting_policy"
            | "waiting_evidence"
            | "waiting_compensation"
            | "completed"
            | "failed"
            | "timed_out"
            | "canceling",
        ) => true,
        ("waiting_policy", "executing" | "failed" | "canceling" | "failed_needs_manual_review") => {
            true
        }
        (
            "waiting_evidence",
            "executing" | "failed" | "canceling" | "failed_needs_manual_review",
        ) => true,
        (
            "waiting_compensation",
            "executing" | "failed" | "failed_needs_manual_review" | "canceling",
        ) => true,
        ("canceling", "canceled") => true,
        // Explicit retry requeues a finished task for another attempt.
        ("failed" | "failed_needs_manual_review" | "timed_out" | "canceled", "queued") => true,
        _ => false,
    }
}

/// Validate a transition, returning a human reason on rejection.
pub fn check_transition(from: &str, to: &str) -> Result<(), String> {
    if can_transition(from, to) {
        Ok(())
    } else {
        Err(format!("invalid transition {from} -> {to}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_states_are_explicit() {
        for terminal in [
            "completed",
            "failed",
            "failed_needs_manual_review",
            "canceled",
            "timed_out",
        ] {
            assert!(is_terminal(terminal), "{terminal} must be terminal");
        }
        for live in [
            "queued",
            "planning",
            "executing",
            "waiting_policy",
            "waiting_evidence",
            "waiting_compensation",
            "canceling",
        ] {
            assert!(!is_terminal(live), "{live} must not be terminal");
        }
    }

    #[test]
    fn invalid_transitions_rejected() {
        for (from, to) in [
            ("queued", "completed"),
            ("completed", "queued"),
            ("executing", "queued"),
            ("canceled", "executing"),
            ("planning", "completed"),
        ] {
            assert!(!can_transition(from, to), "{from} -> {to}");
            assert!(check_transition(from, to).is_err());
        }
    }
}
