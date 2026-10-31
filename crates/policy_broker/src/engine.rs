//! Policy engine: ordered rule composition with default deny.

use chrono::Utc;
use talos_common::now_iso;

use crate::types::{
    ActionRequest, Capability, PolicyDecision, PolicyEvaluation, RiskLevel, SelftestReport,
};

/// Something that approves or blocks agent actions.
pub trait PolicyEngine: std::fmt::Debug + Send + Sync {
    /// Evaluate one request and record a verdict with a human-readable reason.
    ///
    /// Equivalent to [`PolicyEngine::evaluate_with_context`] with an empty
    /// context (no recorded evidence or compensation plans).
    fn evaluate(&self, request: &ActionRequest) -> PolicyEvaluation {
        self.evaluate_with_context(request, &PolicyContext::default())
    }

    /// Evaluate one request against live ledger state.
    ///
    /// `ctx` carries validation-hook results (recorded evidence, approved
    /// compensation plans). Engines must still default-deny anything the
    /// rules do not explicitly allow.
    fn evaluate_with_context(
        &self,
        request: &ActionRequest,
        ctx: &PolicyContext,
    ) -> PolicyEvaluation;
}

/// Ledger facts available to the engine at evaluation time.
#[derive(Debug, Clone, Default)]
pub struct PolicyContext {
    /// Whether evidence backing this action is already recorded.
    pub has_evidence: bool,
    /// Whether a compensation plan already covers this action.
    pub has_compensation_plan: bool,
}

/// Baseline engine: malformed > irreversible > write > read > deny.
#[derive(Debug, Default)]
pub struct DefaultPolicyEngine;

impl DefaultPolicyEngine {
    /// Create the baseline engine.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    fn finish(
        request: &ActionRequest,
        decision: PolicyDecision,
        reason: String,
        required_checks: Vec<String>,
    ) -> PolicyEvaluation {
        PolicyEvaluation {
            request_id: request.id.clone(),
            decision,
            reason,
            required_checks,
            created_at: Utc::now(),
        }
    }
}

/// Capabilities treated as writes (need recorded evidence first).
fn is_write_like(capability: Capability) -> bool {
    matches!(
        capability,
        Capability::WriteFile | Capability::ExecuteCommand | Capability::SystemControl
    )
}

/// Capabilities treated as reads (allowable when low risk).
fn is_read_like(capability: Capability) -> bool {
    matches!(capability, Capability::ReadFile | Capability::OpenBrowser)
}

fn valid_ident(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128
}

impl PolicyEngine for DefaultPolicyEngine {
    fn evaluate_with_context(
        &self,
        request: &ActionRequest,
        ctx: &PolicyContext,
    ) -> PolicyEvaluation {
        // Rule 1 — malformed identifiers: deny.
        if !valid_ident(&request.id)
            || !valid_ident(&request.agent_id)
            || !valid_ident(&request.task_id)
        {
            return Self::finish(
                request,
                PolicyDecision::Deny,
                "denied: malformed identifiers (ids must be 1..=128 chars)".to_owned(),
                vec!["fix_identifiers".to_owned()],
            );
        }
        if request.intent.is_empty() || request.target.is_empty() {
            return Self::finish(
                request,
                PolicyDecision::Deny,
                "denied: intent and target must not be empty".to_owned(),
                vec!["fix_request_fields".to_owned()],
            );
        }

        // Rule 2 — irreversible blast radius: compensation plan required,
        // allowed only once a plan is recorded.
        if request.risk_level == RiskLevel::Irreversible && !ctx.has_compensation_plan {
            return Self::finish(
                request,
                PolicyDecision::RequireCompensation,
                format!(
                    "irreversible {:?} action requires an approved compensation plan before {:?} on {}",
                    request.risk_level, request.capability, request.target
                ),
                vec!["compensation_plan".to_owned(), "human_ack".to_owned()],
            );
        }
        if request.risk_level == RiskLevel::Irreversible {
            return Self::finish(
                request,
                PolicyDecision::Allow,
                format!(
                    "allowed: compensation plan recorded for irreversible {:?} on {}",
                    request.capability, request.target
                ),
                vec!["compensation_plan".to_owned()],
            );
        }

        // Rule 3 — high risk on non-reversible actions escalates to a human.
        if request.risk_level == RiskLevel::High && !request.reversible {
            return Self::finish(
                request,
                PolicyDecision::RequireHumanReview,
                format!(
                    "high-risk non-reversible {:?} on {} requires human review",
                    request.capability, request.target
                ),
                vec!["human_review".to_owned()],
            );
        }

        // Rule 4 — write-like side effects: evidence first, allowed once
        // recorded evidence exists.
        if is_write_like(request.capability) && !ctx.has_evidence {
            return Self::finish(
                request,
                PolicyDecision::RequireEvidence,
                format!(
                    "write-like {:?} on {} requires recorded evidence before execution",
                    request.capability, request.target
                ),
                vec!["record_evidence".to_owned(), "evidence_verified".to_owned()],
            );
        }
        if is_write_like(request.capability) {
            return Self::finish(
                request,
                PolicyDecision::Allow,
                format!(
                    "allowed: recorded evidence satisfies write-like {:?} on {}",
                    request.capability, request.target
                ),
                vec!["evidence_verified".to_owned()],
            );
        }

        // Rule 5 — low-risk reads: allow.
        if request.risk_level == RiskLevel::Low && is_read_like(request.capability) {
            return Self::finish(
                request,
                PolicyDecision::Allow,
                format!(
                    "allowed: low-risk read {:?} on {}",
                    request.capability, request.target
                ),
                vec![],
            );
        }

        // Rule 6 — default deny: nothing else is implicitly trusted.
        Self::finish(
            request,
            PolicyDecision::Deny,
            format!(
                "denied: no allow-rule covers {:?} with {:?} risk (default deny)",
                request.capability, request.risk_level
            ),
            vec!["request_review".to_owned()],
        )
    }
}

/// Parse raw JSON into an [`ActionRequest`] and evaluate it.
///
/// Unknown capabilities (or any malformed payload) fail to parse, and a
/// parse failure is itself a deny-evaluation — unknown actions never pass.
#[must_use]
pub fn evaluate_request_json(
    engine: &dyn PolicyEngine,
    raw: &serde_json::Value,
) -> PolicyEvaluation {
    match serde_json::from_value::<ActionRequest>(raw.clone()) {
        Ok(request) => engine.evaluate(&request),
        Err(err) => PolicyEvaluation {
            request_id: raw
                .get("id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown")
                .to_owned(),
            decision: PolicyDecision::Deny,
            reason: format!("denied: request failed validation ({err})"),
            required_checks: vec!["fix_request_fields".to_owned()],
            created_at: Utc::now(),
        },
    }
}

fn sample_request(
    id: &str,
    capability: Capability,
    risk: RiskLevel,
    reversible: bool,
) -> ActionRequest {
    ActionRequest {
        id: id.to_owned(),
        agent_id: "agent-1".to_owned(),
        task_id: "task-1".to_owned(),
        capability,
        intent: "selftest probe".to_owned(),
        target: "/tmp/talos-selftest".to_owned(),
        payload_json: serde_json::json!({}),
        risk_level: risk,
        reversible,
        requested_at: Utc::now(),
    }
}

/// Shared selftest used by the binary and by `cargo test`.
///
/// Spins up an isolated in-memory evidence database, evaluates one sample
/// per core rule, asserts the expected verdicts, and records PASS/FAIL rows.
pub async fn run_selftest() -> SelftestReport {
    let engine = DefaultPolicyEngine::new();
    let mut checks: Vec<(String, bool, String)> = Vec::new();

    // Selftest touches the evidence store so wiring faults surface here.
    let db_status = match evidence_store::EvidenceDb::connect_memory().await {
        Ok(db) => {
            let insert = evidence_store::insert_event(
                &db,
                evidence_store::NewEvent {
                    id: "selftest-evt".to_owned(),
                    kind: "selftest".to_owned(),
                    payload_json: serde_json::json!({ "at": now_iso() }),
                },
            )
            .await;
            match insert {
                Ok(_) => (true, "temp sqlite round-trip ok".to_owned()),
                Err(err) => (false, format!("insert failed: {err}")),
            }
        }
        Err(err) => (false, format!("db connect failed: {err}")),
    };
    checks.push(("sqlite_selftest_db".to_owned(), db_status.0, db_status.1));

    let cases: Vec<(&str, ActionRequest, PolicyDecision)> = vec![
        (
            "low_risk_read_allowed",
            sample_request("req-read", Capability::ReadFile, RiskLevel::Low, true),
            PolicyDecision::Allow,
        ),
        (
            "write_requires_evidence",
            sample_request("req-write", Capability::WriteFile, RiskLevel::Medium, true),
            PolicyDecision::RequireEvidence,
        ),
        (
            "irreversible_requires_compensation",
            sample_request(
                "req-irreversible",
                Capability::ExecuteCommand,
                RiskLevel::Irreversible,
                false,
            ),
            PolicyDecision::RequireCompensation,
        ),
        (
            "malformed_ids_denied",
            ActionRequest {
                id: String::new(),
                ..sample_request("req-x", Capability::ReadFile, RiskLevel::Low, true)
            },
            PolicyDecision::Deny,
        ),
    ];
    for (name, request, expected) in cases {
        let evaluation = engine.evaluate(&request);
        let ok = evaluation.decision == expected;
        let detail = format!("expected {expected:?}, got {:?}", evaluation.decision);
        checks.push((name.to_owned(), ok, detail));
    }

    // Unknown capability strings must not parse — and must deny.
    let unknown = serde_json::json!({
        "id": "req-unknown",
        "agent_id": "agent-1",
        "task_id": "task-1",
        "capability": "drop_database",
        "intent": "evil",
        "target": "prod",
        "payload_json": {},
        "risk_level": "low",
        "reversible": true,
        "requested_at": now_iso(),
    });
    let evaluation = evaluate_request_json(&engine, &unknown);
    let ok = evaluation.decision == PolicyDecision::Deny;
    checks.push((
        "unknown_capability_denied".to_owned(),
        ok,
        format!("got {:?}", evaluation.decision),
    ));

    let passed = checks.iter().filter(|(_, ok, _)| *ok).count();
    SelftestReport {
        total: checks.len(),
        passed,
        checks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> DefaultPolicyEngine {
        DefaultPolicyEngine::new()
    }

    #[test]
    fn low_risk_read_is_allowed() {
        let evaluation = engine().evaluate(&sample_request(
            "r1",
            Capability::ReadFile,
            RiskLevel::Low,
            true,
        ));
        assert_eq!(evaluation.decision, PolicyDecision::Allow);
        assert!(evaluation.reason.contains("allowed"));
    }

    #[test]
    fn write_requires_evidence() {
        for capability in [
            Capability::WriteFile,
            Capability::ExecuteCommand,
            Capability::SystemControl,
        ] {
            let evaluation =
                engine().evaluate(&sample_request("r2", capability, RiskLevel::Medium, true));
            assert_eq!(
                evaluation.decision,
                PolicyDecision::RequireEvidence,
                "capability {capability:?}"
            );
            assert!(evaluation
                .required_checks
                .contains(&"record_evidence".to_owned()));
        }
    }

    #[test]
    fn irreversible_requires_compensation() {
        let evaluation = engine().evaluate(&sample_request(
            "r3",
            Capability::ExecuteCommand,
            RiskLevel::Irreversible,
            true,
        ));
        assert_eq!(evaluation.decision, PolicyDecision::RequireCompensation);
    }

    #[test]
    fn malformed_request_is_denied() {
        let bad = ActionRequest {
            id: String::new(),
            ..sample_request("r4", Capability::ReadFile, RiskLevel::Low, true)
        };
        let evaluation = engine().evaluate(&bad);
        assert_eq!(evaluation.decision, PolicyDecision::Deny);
        assert!(evaluation.reason.contains("malformed"));
    }

    #[test]
    fn unknown_capability_json_is_denied() {
        let raw = serde_json::json!({
            "id": "r5",
            "agent_id": "agent-1",
            "task_id": "task-1",
            "capability": "teleport",
            "intent": "x",
            "target": "y",
            "payload_json": {},
            "risk_level": "low",
            "reversible": true,
            "requested_at": now_iso(),
        });
        let evaluation = evaluate_request_json(&engine(), &raw);
        assert_eq!(evaluation.decision, PolicyDecision::Deny);
    }

    #[test]
    fn default_deny_for_unmatched() {
        // Medium-risk browser open matches no allow rule.
        let evaluation = engine().evaluate(&sample_request(
            "r6",
            Capability::OpenBrowser,
            RiskLevel::Medium,
            true,
        ));
        assert_eq!(evaluation.decision, PolicyDecision::Deny);
    }

    #[test]
    fn context_gates_write_behind_recorded_evidence() {
        let request = sample_request("r7", Capability::WriteFile, RiskLevel::Medium, true);
        let denied = engine().evaluate(&request);
        assert_eq!(denied.decision, PolicyDecision::RequireEvidence);
        let allowed = engine().evaluate_with_context(
            &request,
            &PolicyContext {
                has_evidence: true,
                has_compensation_plan: false,
            },
        );
        assert_eq!(allowed.decision, PolicyDecision::Allow);
        assert!(allowed.reason.contains("recorded evidence"));
    }

    #[test]
    fn context_gates_irreversible_behind_compensation_plan() {
        let request = sample_request(
            "r8",
            Capability::ExecuteCommand,
            RiskLevel::Irreversible,
            false,
        );
        let gated = engine().evaluate(&request);
        assert_eq!(gated.decision, PolicyDecision::RequireCompensation);
        let allowed = engine().evaluate_with_context(
            &request,
            &PolicyContext {
                has_evidence: false,
                has_compensation_plan: true,
            },
        );
        assert_eq!(allowed.decision, PolicyDecision::Allow);
        assert!(allowed.reason.contains("compensation plan"));
    }

    #[tokio::test]
    async fn selftest_passes() {
        let report = run_selftest().await;
        assert_eq!(report.passed, report.total, "{report:?}");
        assert!(report.ok());
    }
}
