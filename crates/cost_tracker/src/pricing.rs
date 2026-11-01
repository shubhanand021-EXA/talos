//! Unit pricing: real-time cost calculation per resource kind.

/// Cost attribution categories (stored in `cost_records.cost_type`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CostType {
    /// LLM provider call (tokens × model price).
    Llm,
    /// Tool execution (duration × compute rate).
    Tool,
    /// Browser session (duration × browser rate).
    Browser,
    /// Computer-use action (count × per-action rate).
    ComputerUse,
}

impl CostType {
    /// Stable string for storage.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Llm => "llm",
            Self::Tool => "tool",
            Self::Browser => "browser",
            Self::ComputerUse => "computer_use",
        }
    }

    /// Parse a stored string.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "llm" => Some(Self::Llm),
            "tool" => Some(Self::Tool),
            "browser" => Some(Self::Browser),
            "computer_use" => Some(Self::ComputerUse),
            _ => None,
        }
    }
}

/// USD per millisecond of generic tool compute (~$0.36/hour).
pub const COMPUTE_RATE_PER_MS: f64 = 0.000_000_1;
/// USD per millisecond of browser automation (~$0.72/hour: browser + egress).
pub const BROWSER_RATE_PER_MS: f64 = 0.000_000_2;
/// USD per computer-use action (vision + verification overhead).
pub const COMPUTER_USE_RATE_PER_ACTION: f64 = 0.000_5;

/// LLM cost: linear tokens × catalog price (mirrors `llm_gateway`).
#[must_use]
pub fn llm_cost_usd(
    prompt_tokens: u32,
    completion_tokens: u32,
    cost_per_1k_input: f64,
    cost_per_1k_output: f64,
) -> f64 {
    (f64::from(prompt_tokens) / 1_000.0) * cost_per_1k_input
        + (f64::from(completion_tokens) / 1_000.0) * cost_per_1k_output
}

/// Tool execution cost: duration × compute rate.
#[must_use]
pub fn tool_cost_usd(duration_ms: u64) -> f64 {
    duration_ms as f64 * COMPUTE_RATE_PER_MS
}

/// Browser session cost: duration × browser rate.
#[must_use]
pub fn browser_cost_usd(duration_ms: u64) -> f64 {
    duration_ms as f64 * BROWSER_RATE_PER_MS
}

/// Computer-use cost: action count × per-action rate.
#[must_use]
pub fn computer_use_cost_usd(action_count: u64) -> f64 {
    action_count as f64 * COMPUTER_USE_RATE_PER_ACTION
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn llm_cost_matches_gateway_formula() {
        // gpt-4o-mini list rates: 0.00015 in / 0.0006 out per 1k.
        let cost = llm_cost_usd(1_000, 1_000, 0.00015, 0.0006);
        assert!((cost - 0.00075).abs() < 1e-12);
        assert_eq!(llm_cost_usd(0, 0, 1.0, 1.0), 0.0);
    }

    #[test]
    fn compute_rates_scale_linearly() {
        assert!((tool_cost_usd(10_000) - 0.001).abs() < 1e-12);
        assert!((browser_cost_usd(10_000) - 0.002).abs() < 1e-12);
        assert!((computer_use_cost_usd(4) - 0.002).abs() < 1e-12);
    }

    #[test]
    fn cost_type_round_trip() {
        for kind in [
            CostType::Llm,
            CostType::Tool,
            CostType::Browser,
            CostType::ComputerUse,
        ] {
            assert_eq!(CostType::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(CostType::parse("bogus"), None);
    }
}
