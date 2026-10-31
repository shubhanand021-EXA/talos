//! Talos computer-use safety layer: policy-gated, evidence-producing,
//! dry-run-first desktop interaction with no real OS control enabled.
//!
//! Rust owns the safety authority: [`safety::SafetyEnvelope`] enforces the
//! safety envelope (global enable flag, dry-run default, rate limit,
//! coordinate bounds, confidence threshold, dangerous zones, emergency stop)
//! before any adapter runs. [`mock::MockComputer`] provides a deterministic
//! virtual screen for tests and UI demos — no OS calls. [`real`] exposes the
//! [`real::RealComputerAdapter`] trait plus a disabled stub; active
//! keyboard/mouse control is explicitly not implemented (see ADR 0003 and
//! the `real-os-control` feature gate, which fails the build by design).

pub mod mock;
pub mod model;
pub mod real;
pub mod safety;

pub use mock::MockComputer;
pub use model::{
    BoundingBox, ComputerAction, ComputerReceipt, ComputerTarget, DangerZone, DangerZoneKind,
    SafetyConfig, SafetyViolation, COMPUTER_CAPTURE_SCREEN, COMPUTER_CLICK, COMPUTER_DOUBLE_CLICK,
    COMPUTER_DRAG, COMPUTER_HOTKEY, COMPUTER_LOCATE_ELEMENT, COMPUTER_MOVE_MOUSE, COMPUTER_SCROLL,
    COMPUTER_TYPE_TEXT,
};
pub use real::{DisabledRealComputer, RealComputerAdapter};
pub use safety::{CheckedAction, SafetyEnvelope, SafetyHandle, SafetyInput, SafetyStatus};
