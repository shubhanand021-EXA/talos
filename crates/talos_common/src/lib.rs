//! Shared Talos primitives: errors, typed IDs, time and serde helpers.
//!
//! Every Talos Rust crate builds on this crate. No layer may redefine
//! these primitives.

mod error;
mod fidelity;
mod ids;
mod time_helpers;
mod tracing_setup;

pub use error::{Result, TalosError};
pub use fidelity::{check_fidelity_claim, ExecutionFidelity, FidelityMismatchError};
pub use ids::{AgentId, CompensationPlanId, EvidenceId, TaskId};
pub use time_helpers::{now_iso, now_utc, parse_iso};
pub use tracing_setup::init_tracing;

/// Serialize a value to compact JSON.
pub fn to_json<T>(value: &T) -> Result<String>
where
    T: serde::Serialize,
{
    serde_json::to_string(value).map_err(TalosError::Serialization)
}

/// Deserialize a value from JSON.
pub fn from_json<T>(raw: &str) -> Result<T>
where
    T: serde::de::DeserializeOwned,
{
    serde_json::from_str(raw).map_err(TalosError::Serialization)
}
