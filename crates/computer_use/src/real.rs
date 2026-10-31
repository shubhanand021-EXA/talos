//! Real computer-control adapter: trait plus an always-disabled stub.
//!
//! No active keyboard/mouse control is implemented. Any future real OS
//! adapter must live behind the explicit `real-os-control` cargo feature
//! **and** a policy gate; the feature is reserved and fails the build by
//! design until a human-approved implementation with its own audit trail
//! lands (see ADR 0003).

use talos_common::{Result, TalosError};

/// Capability probe for a real OS-control backend.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RealCapabilities {
    /// Backend name (e.g. `"win32-input"`, `"macos-cg"`).
    pub backend: String,
    /// Whether pointer control is available.
    pub pointer: bool,
    /// Whether keyboard control is available.
    pub keyboard: bool,
    /// Whether screen capture is available.
    pub capture: bool,
}

/// Interface every real OS-control backend must implement.
///
/// This trait exists so future adapters share one audited surface. No
/// implementation ships today.
pub trait RealComputerAdapter: Send + Sync + std::fmt::Debug {
    /// Backend name for receipts and audit trails.
    fn backend_name(&self) -> &'static str;
    /// Probe capabilities without touching the OS.
    fn capabilities(&self) -> Result<RealCapabilities>;
    /// Move the pointer. Never implemented today.
    fn move_pointer(&self, _x: i64, _y: i64) -> Result<()> {
        Err(TalosError::Storage(
            "real OS control is not implemented (disabled stub)".to_owned(),
        ))
    }
}

/// Always-disabled stub: every call fails closed with a structured reason.
#[derive(Debug, Default)]
pub struct DisabledRealComputer;

impl RealComputerAdapter for DisabledRealComputer {
    fn backend_name(&self) -> &'static str {
        "disabled-stub"
    }

    fn capabilities(&self) -> Result<RealCapabilities> {
        Ok(RealCapabilities {
            backend: "disabled-stub".to_owned(),
            pointer: false,
            keyboard: false,
            capture: false,
        })
    }
}

/// Report that real OS control is unavailable (used by dry-run planners).
#[must_use]
pub fn unavailable_reason() -> String {
    "real OS control is disabled: mock adapter only (see ADR 0003)".to_owned()
}

#[cfg(feature = "real-os-control")]
compile_error!(
    "the `real-os-control` feature is reserved and has no implementation: \
    active keyboard/mouse control requires an explicit human-approved adapter \
    with its own audit trail (see ADR 0003)"
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_reports_no_capabilities() {
        let stub = DisabledRealComputer;
        assert_eq!(stub.backend_name(), "disabled-stub");
        let caps = stub.capabilities().expect("probe works");
        assert!(!caps.pointer && !caps.keyboard && !caps.capture);
        assert!(stub.move_pointer(1, 1).is_err());
        assert!(!unavailable_reason().is_empty());
    }
}
