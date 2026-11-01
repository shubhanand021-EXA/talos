//! Talos computer-use safety layer: policy-gated, evidence-producing,
//! dry-run-first desktop interaction with real OS control behind an explicit
//! feature gate.
//!
//! Rust owns the safety authority: [`safety::SafetyEnvelope`] enforces the
//! safety envelope (global enable flag, dry-run default, rate limit,
//! coordinate bounds, confidence threshold, dangerous zones, session
//! isolation, emergency stop) before any adapter runs. [`mock::MockComputer`]
//! provides a deterministic virtual screen for tests and UI demos.
//! [`real`] exposes the [`real::RealComputerAdapter`] trait, the always-off
//! [`real::DisabledRealComputer`] stub, and — only with the explicit
//! `real-os-control` cargo feature — [`real::RealComputer`], the real OS
//! accessibility driver with the screenshot/vision loop (see ADR 0008).
//! The feature is never enabled in CI or default builds.

pub mod artifacts;
pub mod driver;
pub mod focus;
pub mod injection;
pub mod mock;
pub mod model;
pub mod real;
#[cfg(feature = "real-os-control")]
pub(crate) mod real_impl;
pub mod safety;
pub mod session;
pub mod vision;

#[cfg(feature = "real-os-control")]
pub mod platform;

#[cfg(feature = "real-os-control")]
pub use platform::PlatformDriver;

pub use artifacts::{artifact_dir, load_screenshot, store_screenshot, StoredArtifact};
pub use driver::{
    AccessibilityDriver, DriverAction, ElementInfo, ElementNode, WindowBounds, WindowInfo,
};
pub use focus::{
    classify_move, manual_movement_detected, FocusEvent, FocusMonitor, KILL_SWITCH_TOLERANCE_PX,
};
pub use injection::{InjectionGuard, InjectionToken};
pub use mock::MockComputer;
pub use model::{
    BoundingBox, ComputerAction, ComputerReceipt, ComputerTarget, DangerZone, DangerZoneKind,
    SafetyConfig, SafetyViolation, COMPUTER_CAPTURE_SCREEN, COMPUTER_CLICK, COMPUTER_DOUBLE_CLICK,
    COMPUTER_DRAG, COMPUTER_HOTKEY, COMPUTER_LOCATE_ELEMENT, COMPUTER_MOVE_MOUSE, COMPUTER_SCROLL,
    COMPUTER_TYPE_TEXT,
};
#[cfg(feature = "real-os-control")]
pub use real::{
    emergency_release_all, emergency_reset_all, real_mode_opted_in, try_execute_live, RealComputer,
};
pub use real::{DisabledRealComputer, RealComputerAdapter};
pub use safety::{CheckedAction, SafetyEnvelope, SafetyHandle, SafetyInput, SafetyStatus};
pub use session::{AppScope, SessionScope, SessionState, ALWAYS_BLOCKED};
pub use vision::{
    compare_frames, hash_bytes, screenshot_from_png, verify_action, FrameComparison, Screenshot,
    Verification,
};
