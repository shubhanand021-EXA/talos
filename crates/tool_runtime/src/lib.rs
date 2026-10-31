//! Talos tool integration framework: registered, schema-validated,
//! policy-gated, evidence-producing, and compensable tools.
//!
//! Five deterministic built-ins ship by default ([`model::BUILT_IN_TOOLS`]).
//! Every invocation flows through [`executor::invoke`]: validate →
//! enabled-check → `ActionRequest` → audit → policy evaluate with live ledger
//! context → deny/disabled short-circuit with structured errors → adapter
//! (dry-run default for risky tools) → receipt persisted as evidence.
//! `fs.write` creates a `backup_restore` compensation plan before mutating.
//! `process.exec` is disabled by default and allowlisted when enabled.

pub mod adapters;
pub mod executor;
pub mod model;
pub mod registry;
pub mod tools_browser;
pub mod tools_exec;
pub mod tools_fs;

pub use adapters::{ExecutionMode, SandboxAdapter, SandboxConfig};
pub use executor::{invoke, GatedOutcome};
pub use model::{
    capability_for, risk_for, validate_ids, ToolErrorDetail, ToolInvocation, ToolManifest,
    ToolReceipt, ToolStatus, BUILT_IN_TOOLS, FS_LIST, FS_READ, FS_SEARCH, FS_WRITE, PROCESS_EXEC,
};
pub use registry::{validate_params, ToolRegistry, ValidatedParams};
