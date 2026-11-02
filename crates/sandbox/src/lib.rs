//! OS sandbox profiles for shell/process execution.
//!
//! Three profiles, enforced at the OS level (never policy checks alone):
//!
//! - [`SandboxProfile::ReadOnly`]: no process execution at all (reads only;
//!   `process.exec` / `code.*` are denied before spawning).
//! - [`SandboxProfile::WorkspaceWrite`]: execution inside an OS sandbox
//!   (Linux bubblewrap namespaces, macOS Seatbelt, Windows Job Objects).
//! - [`SandboxProfile::DangerFullAccess`]: full user access, no OS
//!   confinement — requires an explicit human approval granted upstream
//!   (the [`SandboxConfig::danger_approval_id`] must name it).
//!
//! Fail closed: [`enforce_or_deny`] returns a `sandbox_unavailable` error
//! instead of running unsandboxed whenever the platform backend cannot be
//! established. Test hooks (`TALOS_SANDBOX_FORCE_UNAVAILABLE`,
//! `TALOS_BWRAP_BIN`, `TALOS_CGROUP_ROOT`) make the unavailable path
//! deterministic.

use talos_common::{Result, TalosError};

mod enforce;
pub mod linux;
pub mod macos;
mod profile;
#[cfg(windows)]
pub mod windows;

pub use enforce::{
    enforce_or_deny, sandbox_backends, sandbox_status, select_enforcer, CommandSpec, PostSpawnGuard,
    SandboxEnforcer, SANDBOX_UNAVAILABLE,
};
pub use profile::{network_policy, SandboxConfig, SandboxProfile, SandboxReceipt};

fn err(msg: &str) -> TalosError {
    TalosError::Storage(msg.to_owned())
}
