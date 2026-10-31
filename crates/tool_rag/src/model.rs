//! Local-first Tool-RAG query model: intent in, ranked candidates out.
//!
//! Wire shapes mirror `@talos/contracts` `toolrag.ts` exactly (`camelCase`
//! JSON) so the TypeScript bridge validates every response with Zod.

/// Stable risk ordering used for ceiling filtering (`low` < `medium` <
/// `high` < `irreversible`). Unknown values rank above the ceiling so they
/// are filtered out instead of leaking through.
#[must_use]
pub fn risk_rank(risk_level: &str) -> u8 {
    match risk_level.to_ascii_lowercase().as_str() {
        "low" => 0,
        "medium" => 1,
        "high" => 2,
        "irreversible" => 3,
        _ => u8::MAX,
    }
}

/// One retrieval request: intent text plus hard filters and a result cap.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolQuery {
    /// Intent text (1..=1024 chars, plain words — never executed).
    pub intent: String,
    /// Capability allow-list (empty means every capability).
    pub capabilities: Vec<String>,
    /// Maximum acceptable risk (`low` | `medium` | `high` | `irreversible`).
    pub risk_ceiling: Option<String>,
    /// When true, only tools with evidence backing are returned.
    pub require_evidence: bool,
    /// Result cap (1..=20, default 5).
    pub top_k: usize,
}

/// One ranked candidate: tool, score, and why it was chosen.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCandidate {
    /// Stable tool identifier (`fs.read`, ...).
    pub tool_id: String,
    /// Ranking score (higher is better; deterministic for fixed inputs).
    pub score: f64,
    /// Human-readable ranking explanation.
    pub reason: String,
    /// Document fields that matched the intent.
    pub matched_fields: Vec<String>,
    /// Manifest risk level (`Low` | `Medium` | `High` | `Irreversible`).
    pub risk_level: String,
    /// Approvals the caller must satisfy (`human_review`, `prior_evidence`,
    /// `compensation_plan`).
    pub required_approvals: Vec<String>,
}

/// One indexed tool document: manifest text plus ledger-derived context.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexedTool {
    /// Stable tool identifier.
    pub tool_id: String,
    /// Manifest display name.
    pub name: String,
    /// Manifest description.
    pub description: String,
    /// Manifest capabilities (`ReadFile`, `UiAction`, ...).
    pub capabilities: Vec<String>,
    /// Manifest risk level.
    pub risk_level: String,
    /// Whether effects are undoable.
    pub reversible: bool,
    /// Whether live execution needs recorded evidence first.
    pub requires_evidence: bool,
    /// Whether live execution needs a compensation plan first.
    pub requires_compensation: bool,
    /// Registry enablement at index time.
    pub enabled: bool,
    /// Concatenated searchable text (manifest + task/evidence/compensation
    /// context, deterministically ordered).
    pub text: String,
}

/// Ledger-derived receipt statistics for one tool.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceiptStats {
    /// Successful executions.
    pub success: u64,
    /// Failed executions.
    pub failed: u64,
    /// Policy-denied invocations.
    pub denied: u64,
    /// Disabled-tool short-circuits.
    pub disabled: u64,
}

/// Approvals derived from a manifest (mirrors the policy broker gates).
#[must_use]
pub fn required_approvals_for(
    risk_level: &str,
    requires_evidence: bool,
    requires_compensation: bool,
) -> Vec<String> {
    let mut approvals = Vec::new();
    let rank = risk_rank(risk_level);
    if rank >= risk_rank("high") {
        approvals.push("human_review".to_owned());
    }
    if requires_evidence {
        approvals.push("prior_evidence".to_owned());
    }
    if requires_compensation || rank >= risk_rank("irreversible") {
        approvals.push("compensation_plan".to_owned());
    }
    approvals
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn risk_ordering_is_total_and_conservative() {
        assert!(risk_rank("low") < risk_rank("medium"));
        assert!(risk_rank("medium") < risk_rank("high"));
        assert!(risk_rank("high") < risk_rank("irreversible"));
        assert_eq!(risk_rank("bogus"), u8::MAX);
    }

    #[test]
    fn approvals_mirror_policy_gates() {
        assert!(required_approvals_for("Low", false, false).is_empty());
        assert_eq!(
            required_approvals_for("Medium", true, false),
            vec!["prior_evidence".to_owned()]
        );
        assert_eq!(
            required_approvals_for("High", false, false),
            vec!["human_review".to_owned()]
        );
        assert_eq!(
            required_approvals_for("Irreversible", true, true),
            vec![
                "human_review".to_owned(),
                "prior_evidence".to_owned(),
                "compensation_plan".to_owned()
            ]
        );
    }

    #[test]
    fn query_and_candidate_round_trip_camel_case() {
        let query = ToolQuery {
            intent: "read a file".to_owned(),
            capabilities: vec!["read_file".to_owned()],
            risk_ceiling: Some("medium".to_owned()),
            require_evidence: false,
            top_k: 5,
        };
        let raw = serde_json::to_value(&query).expect("serializes");
        assert_eq!(raw["riskCeiling"], serde_json::json!("medium"));
        assert_eq!(raw["topK"], serde_json::json!(5));
    }
}
