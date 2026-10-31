//! Unified Talos error type.

/// All recoverable Talos failures.
#[derive(Debug, thiserror::Error)]
pub enum TalosError {
    /// An identifier failed validation.
    #[error("invalid id ({what}): {reason}")]
    InvalidId {
        /// Which identifier was rejected.
        what: &'static str,
        /// Human-readable cause.
        reason: String,
    },

    /// JSON (de)serialization failed.
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Storage backend failed.
    #[error("storage error: {0}")]
    Storage(String),

    /// Policy evaluation failed.
    #[error("policy error: {0}")]
    Policy(String),

    /// Time parsing failed.
    #[error("time error: {0}")]
    Time(String),

    /// Catch-all for unexpected failures.
    #[error("internal error: {0}")]
    Internal(String),
}

/// Convenience alias used across all Talos crates.
pub type Result<T> = std::result::Result<T, TalosError>;
