//! Execution fidelity: the typed distinction between simulation and reality.
//!
//! Every Talos layer (agents, tools, ledger, UI, audit swarm) uses these
//! primitives — no layer may redefine them, and no layer may silently
//! upgrade a result: claiming `Live` or `Verified` for `Mock` work is a
//! [`FidelityMismatchError`].

use std::fmt::{Display, Formatter};

/// How much reality backs a result.
///
/// - `Mock`: deterministic simulation, no real side effects.
/// - `Live`: real execution through the tool runtime, real side effects.
/// - `LiveRemote`: real execution on a BYOC remote runner (SSH/Docker),
///   real side effects on the remote — never confusable with local `Live`.
/// - `Verified`: live execution plus independent verification (a test
///   process exited 0, a hash verified against the ledger, output validated).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExecutionFidelity {
    /// Simulation only.
    Mock,
    /// Real execution, real side effects.
    Live,
    /// Real execution on a remote runner, real effects on the remote.
    #[serde(rename = "live_remote")]
    LiveRemote,
    /// Live execution plus independent verification.
    Verified,
}

impl ExecutionFidelity {
    /// Total order: mock < live < live_remote < verified.
    ///
    /// Remote execution is real execution (above local live: it additionally
    /// proves the BYOC path), but still below independent verification.
    #[must_use]
    pub fn rank(self) -> u8 {
        match self {
            Self::Mock => 0,
            Self::Live => 1,
            Self::LiveRemote => 2,
            Self::Verified => 3,
        }
    }

    /// The weaker of two fidelities (a parent is only as verified as its
    /// least-verified child).
    #[must_use]
    pub fn min(self, other: Self) -> Self {
        if self.rank() <= other.rank() {
            self
        } else {
            other
        }
    }

    /// Wire value (`mock` | `live` | `live_remote` | `verified`).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mock => "mock",
            Self::Live => "live",
            Self::LiveRemote => "live_remote",
            Self::Verified => "verified",
        }
    }

    /// Parse a wire value (case-insensitive, `-` and `_` equivalent).
    pub fn parse(value: &str) -> crate::Result<Self> {
        match value.trim().to_ascii_lowercase().replace('-', "_").as_str() {
            "mock" => Ok(Self::Mock),
            "live" => Ok(Self::Live),
            "live_remote" | "liveremote" => Ok(Self::LiveRemote),
            "verified" => Ok(Self::Verified),
            _ => Err(crate::TalosError::Storage(format!(
                "unknown execution fidelity: {value}"
            ))),
        }
    }
}

impl Display for ExecutionFidelity {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A result presented above its actual fidelity (never panics the caller).
///
/// Returned (never panicked) whenever a caller tries to claim more reality
/// than the work actually has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FidelityMismatchError {
    /// What the work actually was.
    pub actual: ExecutionFidelity,
    /// What was claimed.
    pub claimed: ExecutionFidelity,
}

impl Display for FidelityMismatchError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "fidelity mismatch: {} work presented as {} (actual: {})",
            self.actual.as_str(),
            self.claimed.as_str(),
            self.actual
        )
    }
}

impl std::error::Error for FidelityMismatchError {}

/// Refuse to claim more fidelity than the work has.
///
/// `claimed <= actual` passes; anything stronger is a [`FidelityMismatchError`].
pub fn check_fidelity_claim(
    actual: ExecutionFidelity,
    claimed: ExecutionFidelity,
) -> std::result::Result<(), FidelityMismatchError> {
    if claimed.rank() <= actual.rank() {
        Ok(())
    } else {
        Err(FidelityMismatchError { actual, claimed })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fidelity_orders_mock_below_verified() {
        assert!(ExecutionFidelity::Mock.rank() < ExecutionFidelity::Live.rank());
        assert!(ExecutionFidelity::Live.rank() < ExecutionFidelity::LiveRemote.rank());
        assert!(ExecutionFidelity::LiveRemote.rank() < ExecutionFidelity::Verified.rank());
        assert_eq!(
            ExecutionFidelity::Mock.min(ExecutionFidelity::Verified),
            ExecutionFidelity::Mock
        );
        assert_eq!(
            ExecutionFidelity::Live.min(ExecutionFidelity::Verified),
            ExecutionFidelity::Live
        );
        assert_eq!(
            ExecutionFidelity::Live.min(ExecutionFidelity::LiveRemote),
            ExecutionFidelity::Live
        );
        assert_eq!(
            ExecutionFidelity::parse("LIVE").unwrap(),
            ExecutionFidelity::Live
        );
        assert_eq!(
            ExecutionFidelity::parse("LIVE_REMOTE").unwrap(),
            ExecutionFidelity::LiveRemote
        );
        assert_eq!(
            ExecutionFidelity::parse("live-remote").unwrap(),
            ExecutionFidelity::LiveRemote
        );
        assert!(ExecutionFidelity::parse("real").is_err());
    }

    #[test]
    fn mismatch_rejects_mock_presented_as_verified() {
        assert!(check_fidelity_claim(ExecutionFidelity::Mock, ExecutionFidelity::Mock).is_ok());
        assert!(check_fidelity_claim(ExecutionFidelity::Live, ExecutionFidelity::Mock).is_ok());
        let err = check_fidelity_claim(ExecutionFidelity::Mock, ExecutionFidelity::Verified)
            .expect_err("mock as verified must fail");
        assert_eq!(err.actual, ExecutionFidelity::Mock);
        assert_eq!(err.claimed, ExecutionFidelity::Verified);
        assert!(
            check_fidelity_claim(ExecutionFidelity::Live, ExecutionFidelity::Verified).is_err()
        );
        assert!(
            check_fidelity_claim(ExecutionFidelity::Verified, ExecutionFidelity::Verified).is_ok()
        );
    }

    #[test]
    fn remote_fidelity_never_confusable_with_local() {
        // Remote work may be claimed as plain live (undersell is honest)...
        assert!(
            check_fidelity_claim(ExecutionFidelity::LiveRemote, ExecutionFidelity::Live).is_ok()
        );
        // ...but local work must never be presented as remote.
        let err = check_fidelity_claim(ExecutionFidelity::Live, ExecutionFidelity::LiveRemote)
            .expect_err("local as remote must fail");
        assert_eq!(err.actual, ExecutionFidelity::Live);
        assert_eq!(err.claimed, ExecutionFidelity::LiveRemote);
        assert_eq!(ExecutionFidelity::LiveRemote.as_str(), "live_remote");
    }
}
