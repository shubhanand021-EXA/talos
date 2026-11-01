//! Focus monitoring and the manual-input kill switch.
//!
//! Before injection the driver records the expected window and pointer
//! position. During injection every observation is compared: a focus change
//! pauses automation (`FocusLost`), an unexpected application aborts it, and
//! pointer movement the agent did not cause stops injection immediately
//! (the human took control).

use crate::driver::WindowInfo;

/// Outcome of one focus observation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FocusEvent {
    /// Still on the expected window.
    Steady,
    /// Focus moved between two known windows.
    Moved {
        /// Previously focused window title.
        from: String,
        /// Newly focused window title.
        to: String,
    },
    /// Focus left every known window (desktop, unknown surface).
    FocusLost {
        /// What was expected.
        expected: String,
    },
    /// A blocked or unexpected application took focus.
    UnexpectedApp {
        /// Application that took focus.
        app: String,
        /// Why it is unexpected.
        reason: String,
    },
}

/// Tracks the expected window across an injection sequence.
#[derive(Debug, Clone, Default)]
pub struct FocusMonitor {
    /// Window the agent believes it is driving.
    pub expected: Option<WindowInfo>,
}

impl FocusMonitor {
    /// Start tracking an expected window.
    #[must_use]
    pub fn expecting(window: WindowInfo) -> Self {
        Self {
            expected: Some(window),
        }
    }

    /// Classify the currently observed window against the expectation.
    #[must_use]
    pub fn observe(&self, current: Option<&WindowInfo>) -> FocusEvent {
        match (&self.expected, current) {
            (None, _) => FocusEvent::Steady,
            (Some(_), None) => FocusEvent::FocusLost {
                expected: self
                    .expected
                    .as_ref()
                    .map(|window| window.title.clone())
                    .unwrap_or_default(),
            },
            (Some(expected), Some(current)) => {
                if expected.title == current.title && expected.app_name == current.app_name {
                    FocusEvent::Steady
                } else {
                    FocusEvent::Moved {
                        from: expected.title.clone(),
                        to: current.title.clone(),
                    }
                }
            }
        }
    }
}

/// Kill-switch decision: did the pointer move without Talos causing it?
///
/// `talos_pos` is where the agent last put the pointer; `actual_pos` is
/// where the OS reports it now. A drift beyond `tolerance_px` means a human
/// (or another agent) moved the mouse: stop injection immediately.
#[must_use]
pub fn manual_movement_detected(
    talos_pos: (i64, i64),
    actual_pos: (i64, i64),
    tolerance_px: i64,
) -> bool {
    let dx = talos_pos.0.saturating_sub(actual_pos.0).abs();
    let dy = talos_pos.1.saturating_sub(actual_pos.1).abs();
    dx > tolerance_px || dy > tolerance_px
}

/// Classify a focus move against the session scope.
///
/// A move to a session-blocked application becomes [`FocusEvent::UnexpectedApp`]
/// (fail the verification downstream); anywhere else stays [`FocusEvent::Steady`]
/// because the action itself may have switched windows legitimately.
#[must_use]
pub fn classify_move(
    scope: &crate::session::SessionScope,
    task_id: &str,
    to_app: &str,
) -> FocusEvent {
    if scope.app_allowed(task_id, to_app) {
        FocusEvent::Steady
    } else {
        FocusEvent::UnexpectedApp {
            app: to_app.to_owned(),
            reason: scope.verdict_reason(task_id, to_app),
        }
    }
}

/// Maximum drift tolerated before the kill switch fires (pixels).
pub const KILL_SWITCH_TOLERANCE_PX: i64 = 8;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::WindowBounds;

    fn window(title: &str, app: &str) -> WindowInfo {
        WindowInfo {
            title: title.to_owned(),
            app_name: app.to_owned(),
            process_id: None,
            bounds: WindowBounds {
                x: 0,
                y: 0,
                width: 800,
                height: 600,
            },
        }
    }

    #[test]
    fn steady_when_expectation_matches() {
        let monitor = FocusMonitor::expecting(window("Notes", "notes.exe"));
        assert_eq!(
            monitor.observe(Some(&window("Notes", "notes.exe"))),
            FocusEvent::Steady
        );
    }

    #[test]
    fn moved_when_focus_changes() {
        let monitor = FocusMonitor::expecting(window("Notes", "notes.exe"));
        assert_eq!(
            monitor.observe(Some(&window("Mail", "mail.exe"))),
            FocusEvent::Moved {
                from: "Notes".to_owned(),
                to: "Mail".to_owned()
            }
        );
    }

    #[test]
    fn lost_when_no_window_observed() {
        let monitor = FocusMonitor::expecting(window("Notes", "notes.exe"));
        assert!(matches!(
            monitor.observe(None),
            FocusEvent::FocusLost { .. }
        ));
    }

    #[test]
    fn kill_switch_fires_on_manual_drift() {
        assert!(!manual_movement_detected((100, 100), (104, 102), 8));
        assert!(manual_movement_detected((100, 100), (200, 100), 8));
    }

    #[test]
    fn blocked_apps_classify_as_unexpected() {
        let scope = crate::session::SessionScope::default();
        assert!(matches!(
            classify_move(&scope, "t", "Notes"),
            FocusEvent::Steady
        ));
        assert!(matches!(
            classify_move(&scope, "t", "System Settings"),
            FocusEvent::UnexpectedApp { .. }
        ));
    }
}
