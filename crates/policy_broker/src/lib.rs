//! Talos Policy Broker: the security and orchestration authority.
//!
//! Every agent action with side effects passes through [`DefaultPolicyEngine`].
//! Rules compose in order; the first decisive rule wins, otherwise the
//! request is denied. Unknown capabilities never parse, so they can only
//! arrive via [`evaluate_request_json`], which denies them explicitly.

mod engine;
mod types;

pub use engine::{evaluate_request_json, run_selftest, DefaultPolicyEngine, PolicyEngine};
pub use types::{
    ActionRequest, Capability, PolicyDecision, PolicyEvaluation, RiskLevel, SelftestReport,
};
