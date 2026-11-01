//! Talos tool integration framework: registered, schema-validated,
//! policy-gated, evidence-producing, and compensable tools.
//!
//! Twenty-three deterministic built-ins ship by default ([`model::BUILT_IN_TOOLS`]):
//! filesystem, subprocess, browser-use, and computer-use families.
//! Every invocation flows through [`executor::invoke`]: validate →
//! safety-envelope check (computer tools) → enabled-check → `ActionRequest` →
//! audit → policy evaluate with live ledger context → deny/disabled
//! short-circuit with structured errors → adapter (dry-run default for risky
//! tools) → receipt persisted as evidence.
//! `fs.write` creates a `backup_restore` compensation plan before mutating.
//! `process.exec` is disabled by default and allowlisted when enabled.
//! Computer-use tools additionally enforce the [`computer_use`] safety envelope
//! (global enable flag, emergency stop, rate limit, bounds, zones) and are all
//! disabled by default.
//! Browser tools execute the deterministic mock by default; with
//! `TALOS_BROWSER_LIVE=1` they route to the Node worker
//! ([`browser_worker`]) over JSON-RPC with a 30s kill timeout.

pub mod adapters;
pub mod browser_worker;
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
    ToolReceipt, ToolStatus, BROWSER_CLICK, BROWSER_EXTRACT, BROWSER_NAVIGATE, BROWSER_SCREENSHOT,
    BROWSER_SCROLL, BROWSER_SELECT, BROWSER_SESSION, BROWSER_SNAPSHOT, BROWSER_TYPE,
    BUILT_IN_TOOLS, COMPUTER_CAPTURE_SCREEN, COMPUTER_CLICK, COMPUTER_DOUBLE_CLICK, COMPUTER_DRAG,
    COMPUTER_HOTKEY, COMPUTER_LOCATE_ELEMENT, COMPUTER_MOVE_MOUSE, COMPUTER_SCROLL, COMPUTER_TOOLS,
    COMPUTER_TYPE_TEXT, FS_LIST, FS_READ, FS_SEARCH, FS_WRITE, PROCESS_EXEC,
};
pub use registry::{validate_params, ToolRegistry, ValidatedParams};
