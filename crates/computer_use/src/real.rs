//! Real computer-control adapter: trait plus an always-disabled stub.
//!
//! The real OS driver lives in [`real_impl`] and compiles ONLY with the
//! explicit `real-os-control` cargo feature (never in CI or default builds).
//! Default builds expose [`DisabledRealComputer`] plus [`try_execute_live`]
//! (which always declines) so the rest of the tree links unchanged.

use talos_common::{Result, TalosError};

#[cfg(feature = "real-os-control")]
pub use crate::real_impl::{
    emergency_release_all, emergency_reset_all, real_mode_opted_in, try_execute_live, RealComputer,
};

/// Capability probe for a real OS-control backend.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RealCapabilities {
    /// Backend name (e.g. `"win32-uia"`, `"macos-ax"`, `"linux-atspi"`).
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
/// This trait exists so future adapters share one audited surface.
pub trait RealComputerAdapter: Send + Sync + std::fmt::Debug {
    /// Backend name for receipts and audit trails.
    fn backend_name(&self) -> &'static str;
    /// Probe capabilities without touching the OS.
    fn capabilities(&self) -> Result<RealCapabilities>;
    /// Move the pointer. Only implemented by the real adapter.
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

/// Whether the current process opted into real desktop control.
///
/// Default builds always report false. Feature builds additionally require
/// `TALOS_COMPUTER_REAL=1` in the environment.
#[cfg(not(feature = "real-os-control"))]
#[must_use]
pub fn real_mode_opted_in() -> bool {
    false
}

/// Latch the emergency stop process-wide (default build: no-op counters).
#[cfg(not(feature = "real-os-control"))]
#[must_use]
pub fn emergency_release_all() -> (String, usize) {
    (talos_common::now_iso(), 0)
}

/// Clear a latched stop (default build: no-op).
#[cfg(not(feature = "real-os-control"))]
pub fn emergency_reset_all() {}

/// Route one live computer tool through the real driver when opted in.
///
/// Default builds always return `None` (the caller falls through to the
/// mock). Feature builds without `TALOS_COMPUTER_REAL=1` do the same.
#[cfg(not(feature = "real-os-control"))]
pub async fn try_execute_live(
    _tool_id: &str,
    _task_id: &str,
    _params: &serde_json::Value,
    _root: &std::path::Path,
    _envelope: &crate::safety::SafetyHandle,
) -> Option<Result<serde_json::Value>> {
    None
}

/// Translate a tool call into a resolved driver action + target center.
///
/// Pure and total: unknown tools and missing coordinates fail closed.
/// Runs in default builds (no OS access) so planning stays testable.
pub fn plan_action(
    tool_id: &str,
    params: &serde_json::Value,
) -> Result<(crate::driver::DriverAction, Option<(i64, i64)>)> {
    use crate::driver::DriverAction;
    fn err(msg: &str) -> TalosError {
        TalosError::Storage(msg.to_owned())
    }
    let num = |key: &str| params.get(key).and_then(serde_json::Value::as_i64);
    let center = match (num("x"), num("y")) {
        (Some(x), Some(y)) => Some((x, y)),
        _ => None,
    };
    let action = match tool_id {
        "computer.move_mouse" => {
            let (x, y) = center.ok_or_else(|| err("move_mouse requires x/y"))?;
            DriverAction::MoveTo { x, y }
        }
        "computer.click" => {
            let (x, y) = center.ok_or_else(|| err("click requires x/y or selector"))?;
            let button = params
                .get("button")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("left")
                .to_owned();
            DriverAction::Click { x, y, button }
        }
        "computer.double_click" => {
            let (x, y) = center.ok_or_else(|| err("double_click requires x/y"))?;
            DriverAction::DoubleClick { x, y }
        }
        "computer.drag" => {
            let (x, y) = center.ok_or_else(|| err("drag requires x/y"))?;
            let x2 = num("x2").ok_or_else(|| err("drag requires x2"))?;
            let y2 = num("y2").ok_or_else(|| err("drag requires y2"))?;
            DriverAction::Drag { x, y, x2, y2 }
        }
        "computer.type_text" => {
            let text = params
                .get("text")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| err("type_text requires text"))?;
            DriverAction::TypeText {
                text: text.to_owned(),
            }
        }
        "computer.hotkey" => {
            let keys = params
                .get("keys")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| err("hotkey requires keys"))?;
            DriverAction::Hotkey {
                keys: keys.to_owned(),
            }
        }
        "computer.scroll" => {
            let direction = params
                .get("direction")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("down")
                .to_owned();
            let pixels = num("pixels").unwrap_or(800);
            DriverAction::Scroll {
                x: num("x"),
                y: num("y"),
                direction,
                pixels,
            }
        }
        "computer.capture_screen" | "computer.locate_element" => {
            return Err(err("read-only actions do not inject"));
        }
        other => return Err(err(&format!("unknown computer tool: {other}"))),
    };
    Ok((action, center))
}

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

    #[test]
    fn planned_actions_reject_unknown_tools() {
        let params = serde_json::json!({ "x": 1, "y": 2 });
        assert!(plan_action("computer.click", &params).is_ok());
        assert!(plan_action("computer.nope", &params).is_err());
        assert!(plan_action("computer.capture_screen", &params).is_err());
    }
}
