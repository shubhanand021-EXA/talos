//! Computer-use execution backends: where pointer/keyboard/screen I/O lands.
//!
//! The host OS is no longer a valid target for autonomous agents. Every live
//! computer action routes through one of these backends:
//!
//! - [`ComputerBackend::Mock`] — deterministic virtual screen (default, tests,
//!   UI demos; never touches anything).
//! - [`ComputerBackend::EphemeralVm`] — a per-task Linux desktop container
//!   (see [`crate::vm`]): the only supported live backend. The container is
//!   spawned on demand, sees a read-only workspace mount (never the host
//!   root), and is destroyed afterwards.
//! - [`ComputerBackend::NativeOs`] — **deprecated**: the pre-Batch-6 native
//!   accessibility driver. It compiles only with the `real-os-control` cargo
//!   feature and executes only when the operator holds a `danger_full_access`
//!   human approval (the same single-shot grant as `process.exec` danger
//!   mode). Any other configuration denies with `native_os_deprecated`.
//!
//! Backend selection is explicit (`TALOS_COMPUTER_BACKEND=mock|native|vm`,
//! default `mock`) and reported in receipts (`backend`) and the safety
//! status so the UI can never mistake a mock run for a contained one.

use talos_common::{Result, TalosError};

fn err(msg: impl Into<String>) -> TalosError {
    TalosError::Storage(msg.into())
}

/// Where live computer I/O executes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ComputerBackend {
    /// Deterministic virtual screen (default).
    #[default]
    Mock,
    /// Deprecated native OS driver (danger approval required).
    NativeOs,
    /// Ephemeral per-task Linux desktop container.
    EphemeralVm,
}

impl ComputerBackend {
    /// Wire value (`mock` | `native` | `ephemeral-vm`).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mock => "mock",
            Self::NativeOs => "native-os",
            Self::EphemeralVm => "ephemeral-vm",
        }
    }

    /// Parse a wire/env value (case-insensitive, fail closed).
    pub fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "mock" | "" => Ok(Self::Mock),
            "native" | "native-os" | "native_os" => Ok(Self::NativeOs),
            "vm" | "ephemeral-vm" | "ephemeral_vm" | "ephemeralvm" => Ok(Self::EphemeralVm),
            other => Err(err(format!("unknown computer backend: {other}"))),
        }
    }

    /// Resolve from `TALOS_COMPUTER_BACKEND` (default [`ComputerBackend::Mock`]).
    #[must_use]
    pub fn from_env() -> Self {
        std::env::var("TALOS_COMPUTER_BACKEND")
            .ok()
            .and_then(|raw| Self::parse(&raw).ok())
            .unwrap_or(Self::Mock)
    }

    /// True for the deprecated host-OS driver.
    #[must_use]
    pub fn is_native(self) -> bool {
        matches!(self, Self::NativeOs)
    }
}

impl std::fmt::Display for ComputerBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl serde::Serialize for ComputerBackend {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> std::result::Result<S::Ok, S::Error> {
        ser.serialize_str(self.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for ComputerBackend {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> std::result::Result<Self, D::Error> {
        let raw = String::deserialize(de)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_parsing_is_total_and_case_insensitive() {
        assert_eq!(
            ComputerBackend::parse("mock").expect("mock"),
            ComputerBackend::Mock
        );
        assert_eq!(
            ComputerBackend::parse("VM").expect("vm"),
            ComputerBackend::EphemeralVm
        );
        assert_eq!(
            ComputerBackend::parse("ephemeral-vm").expect("vm"),
            ComputerBackend::EphemeralVm
        );
        assert_eq!(
            ComputerBackend::parse("native").expect("native"),
            ComputerBackend::NativeOs
        );
        assert!(ComputerBackend::parse("host").is_err());
        assert_eq!(ComputerBackend::Mock.as_str(), "mock");
    }

    #[test]
    fn wire_shape_is_kebab_case() {
        let raw = serde_json::to_string(&ComputerBackend::EphemeralVm).expect("json");
        assert_eq!(raw, "\"ephemeral-vm\"");
        let back: ComputerBackend = serde_json::from_str(&raw).expect("parses");
        assert_eq!(back, ComputerBackend::EphemeralVm);
    }
}
