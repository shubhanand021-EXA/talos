//! Platform-neutral accessibility driver contract.
//!
//! Every OS backend ([`crate::platform`]) implements [`AccessibilityDriver`]:
//! focused-window discovery, element-tree enumeration, hit-testing,
//! text extraction, and action execution. Element identification is
//! structured (accessibility APIs) first; coordinate mapping is the
//! documented fallback when a backend cannot resolve structure.

use serde::{Deserialize, Serialize};

use crate::model::BoundingBox;

/// A top-level window: title, owning application, and screen bounds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowInfo {
    /// Window title as reported by the OS.
    pub title: String,
    /// Owning application name (process image name or bundle id).
    pub app_name: String,
    /// OS process identifier, when the backend can resolve it.
    pub process_id: Option<u32>,
    /// Window bounds in screen pixels.
    pub bounds: WindowBounds,
}

/// Screen rectangle for a window (origin top-left, pixels).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowBounds {
    /// Left edge in pixels.
    pub x: i64,
    /// Top edge in pixels.
    pub y: i64,
    /// Width in pixels.
    pub width: i64,
    /// Height in pixels.
    pub height: i64,
}

/// One identified UI element: role, label, geometry, and match confidence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ElementInfo {
    /// Accessibility role (`button`, `textbox`, `link`, ...).
    pub role: String,
    /// Human-readable label (name/title/value, best available).
    pub name: String,
    /// Screen bounds, when the backend can resolve geometry.
    pub bounds: Option<BoundingBox>,
    /// Current text/value content, when readable without side effects.
    pub value: Option<String>,
    /// Backend state flags (e.g. `enabled`, `focused`, `offscreen`).
    pub states: Vec<String>,
    /// Match confidence in `0.0..=1.0`; below the envelope threshold the
    /// element must not be acted on.
    pub confidence: f64,
    /// Whether the element currently accepts input.
    pub interactable: bool,
}

/// A node of the accessibility tree: one element plus bounded children.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ElementNode {
    /// This node's element.
    pub info: ElementInfo,
    /// Direct children (depth-capped by the backend).
    pub children: Vec<ElementNode>,
}

/// Driver-level actions (translated to OS input or accessibility actions).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DriverAction {
    /// Move the pointer to a point.
    MoveTo {
        /// X in screen pixels.
        x: i64,
        /// Y in screen pixels.
        y: i64,
    },
    /// Click a mouse button at a point.
    Click {
        /// X in screen pixels.
        x: i64,
        /// Y in screen pixels.
        y: i64,
        /// `left` | `right` | `middle`.
        button: String,
    },
    /// Double-click at a point.
    DoubleClick {
        /// X in screen pixels.
        x: i64,
        /// Y in screen pixels.
        y: i64,
    },
    /// Press-drag-release.
    Drag {
        /// Start X in screen pixels.
        x: i64,
        /// Start Y in screen pixels.
        y: i64,
        /// End X in screen pixels.
        x2: i64,
        /// End Y in screen pixels.
        y2: i64,
    },
    /// Type text into the focused element.
    TypeText {
        /// Text to type (validated length upstream).
        text: String,
    },
    /// Press a hotkey chord (`ctrl+s`, `alt+F4`, ...).
    Hotkey {
        /// Chord as written by the caller.
        keys: String,
    },
    /// Scroll at a position.
    Scroll {
        /// X in screen pixels (optional).
        x: Option<i64>,
        /// Y in screen pixels (optional).
        y: Option<i64>,
        /// `up` | `down` | `left` | `right`.
        direction: String,
        /// Pixels (backend-clamped).
        pixels: i64,
    },
    /// Activate/focus a window.
    Activate,
}

/// Platform accessibility backend contract.
///
/// Implementations must be total: every method fails closed with a
/// [`talos_common::TalosError`] instead of panicking, and methods that
/// cannot be served on a platform return a structured "unsupported" error
/// so the caller falls back to coordinate mapping.
///
/// Methods are async because D-Bus backends (Linux) are async; blocking
/// backends (UI Automation, AXUIElement) complete synchronously inside.
// The `async_fn_in_trait` lint wants `Send` bounds on the futures; this
// trait is crate-internal and its futures never cross threads (blocking
// backends resolve inline, D-Bus backends are driven on fresh runtimes),
// so the default bounds are exactly what we want.
#[allow(async_fn_in_trait)]
pub trait AccessibilityDriver: Send + Sync + std::fmt::Debug {
    /// Backend name (`win32-uia`, `macos-ax`, `linux-atspi`, `fallback`).
    fn backend_name(&self) -> &'static str;
    /// Window that currently holds input focus.
    async fn focused_window(&self) -> talos_common::Result<WindowInfo>;
    /// Accessibility subtree rooted at `window`, depth-capped by the backend.
    async fn element_tree(
        &self,
        window: &WindowInfo,
        max_depth: usize,
    ) -> talos_common::Result<ElementNode>;
    /// Element at a screen point (accessibility hit-test, then fallback).
    async fn element_at(&self, x: i64, y: i64) -> talos_common::Result<ElementInfo>;
    /// Read-only text content of an element (never types, never clicks).
    async fn text_of(&self, element: &ElementInfo) -> talos_common::Result<String>;
    /// Focus an element's window (best-effort; injection still verifies).
    async fn focus_element(&self, element: &ElementInfo) -> talos_common::Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_and_element_shapes_round_trip() {
        let window = WindowInfo {
            title: "Notes".to_owned(),
            app_name: "notes.exe".to_owned(),
            process_id: Some(42),
            bounds: WindowBounds {
                x: 0,
                y: 0,
                width: 1440,
                height: 900,
            },
        };
        let raw = serde_json::to_string(&window).expect("serializes");
        let back: WindowInfo = serde_json::from_str(&raw).expect("parses");
        assert_eq!(back, window);
        let element = ElementInfo {
            role: "button".to_owned(),
            name: "OK".to_owned(),
            bounds: None,
            value: None,
            states: vec!["enabled".to_owned()],
            confidence: 0.9,
            interactable: true,
        };
        assert!(element.interactable);
    }
}
