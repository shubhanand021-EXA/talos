//! Budget allocation for the Context Assembly Engine.
//!
//! Starting targets come from the Perplexity allocation model (20/25/35/10/10
//! over the input budget after a 15% output reservation). All values live in
//! `.talos/context.config.json` and are validated to sum to 100%.

use serde::{Deserialize, Serialize};

/// Percentage split of the input budget across assembly buckets.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssemblyAllocation {
    /// Policy/task contracts: safety rules, task spec, constraints (20%).
    pub contracts_pct: u8,
    /// Recent tool state: receipts, file states (25%).
    pub tool_state_pct: u8,
    /// Evidence: verified claims, receipts, chain status (35%).
    pub evidence_pct: u8,
    /// Memory: relevant facts, prior decisions (10%).
    pub memory_pct: u8,
    /// Plan/summary: current step, next actions (10%).
    pub plan_pct: u8,
}

impl AssemblyAllocation {
    /// True when the five buckets sum to exactly 100.
    #[must_use]
    pub fn sums_to_100(&self) -> bool {
        self.contracts_pct as u16
            + self.tool_state_pct as u16
            + self.evidence_pct as u16
            + self.memory_pct as u16
            + self.plan_pct as u16
            == 100
    }
}

impl Default for AssemblyAllocation {
    fn default() -> Self {
        Self {
            contracts_pct: 20,
            tool_state_pct: 25,
            evidence_pct: 35,
            memory_pct: 10,
            plan_pct: 10,
        }
    }
}

/// Assembly knobs: allocation, output reservation, template version.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextAssemblyConfig {
    /// Bucket split of the input budget.
    pub allocation: AssemblyAllocation,
    /// Output/headroom reservation in percent of the model window (15%).
    pub reserved_output_pct: u8,
    /// Prompt template version stamped on every assembly.
    pub template_version: String,
    /// Fallback context window when the model is unknown.
    pub default_context_window: u32,
}

impl Default for ContextAssemblyConfig {
    fn default() -> Self {
        Self {
            allocation: AssemblyAllocation::default(),
            reserved_output_pct: 15,
            template_version: crate::template::TEMPLATE_VERSION.to_owned(),
            default_context_window: 128_000,
        }
    }
}

impl ContextAssemblyConfig {
    /// Validate allocation sums and reservation bounds.
    pub fn validate(&self) -> Result<(), String> {
        if !self.allocation.sums_to_100() {
            return Err("allocation buckets must sum to 100".to_owned());
        }
        if self.reserved_output_pct > 50 {
            return Err("reserved_output_pct must be within 0..=50".to_owned());
        }
        if self.template_version.is_empty() || self.template_version.len() > 64 {
            return Err("template_version must be 1..=64 chars".to_owned());
        }
        Ok(())
    }

    /// Input budget after the output reservation.
    #[must_use]
    pub fn input_budget(&self, context_window: u32) -> u32 {
        let reserved = context_window.saturating_mul(self.reserved_output_pct as u32) / 100;
        context_window.saturating_sub(reserved)
    }

    /// Token target for one bucket.
    #[must_use]
    pub fn bucket_target(&self, input_budget: u32, bucket_pct: u8) -> u32 {
        input_budget.saturating_mul(bucket_pct as u32) / 100
    }

    /// Load the versioned repo default (mirrors `.talos/context.config.json`).
    pub fn load_repo_default() -> Result<Self, String> {
        let path = format!(
            "{}/../../.talos/context.config.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let raw =
            std::fs::read_to_string(&path).map_err(|err| format!("read context config: {err}"))?;
        let config: Self =
            serde_json::from_str(&raw).map_err(|err| format!("parse context config: {err}"))?;
        config.validate()?;
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_sum_to_100() {
        let config = ContextAssemblyConfig::default();
        assert!(config.allocation.sums_to_100());
        assert!(config.validate().is_ok());
        assert_eq!(config.input_budget(100_000), 85_000);
        assert_eq!(config.bucket_target(85_000, 20), 17_000);
    }

    #[test]
    fn rejects_bad_allocations() {
        let allocation = AssemblyAllocation {
            plan_pct: 11,
            ..AssemblyAllocation::default()
        };
        let config = ContextAssemblyConfig {
            allocation,
            ..ContextAssemblyConfig::default()
        };
        assert!(!config.allocation.sums_to_100());
        assert!(config.validate().is_err());
        let uncapped = ContextAssemblyConfig {
            reserved_output_pct: 51,
            ..ContextAssemblyConfig::default()
        };
        assert!(uncapped.validate().is_err());
    }

    #[test]
    fn repo_default_matches_embedded_default() {
        let file = ContextAssemblyConfig::load_repo_default().unwrap();
        assert_eq!(file, ContextAssemblyConfig::default());
    }
}
