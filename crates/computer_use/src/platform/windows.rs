//! Windows accessibility driver: UI Automation element tree + active-window
//! focus, with coordinate-mapping fallback.
//!
//! [`WindowsDriver`] creates a fresh `UIAutomation` client per call (COM is
//! initialized per call by the crate; no handles escape the call). All
//! failures degrade to coordinate mapping: an element at a point is always
//! resolvable, just with lower confidence.

use talos_common::{Result, TalosError};

use crate::driver::{AccessibilityDriver, ElementInfo, ElementNode, WindowBounds, WindowInfo};
use crate::model::BoundingBox;

fn err(msg: &str) -> TalosError {
    TalosError::Storage(msg.to_owned())
}

/// UI Automation backend for Windows (`win32-uia`).
#[derive(Debug, Default)]
pub struct WindowsDriver;

impl WindowsDriver {
    /// Create the driver (stateless; clients are built per call).
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Focused window via the cross-platform active-window probe.
    fn active_window() -> Result<WindowInfo> {
        let active = active_win_pos_rs::get_active_window()
            .map_err(|_| err("active window probe failed"))?;
        Ok(WindowInfo {
            title: active.title,
            app_name: active.app_name,
            process_id: None,
            bounds: WindowBounds {
                x: active.position.x as i64,
                y: active.position.y as i64,
                width: active.position.width as i64,
                height: active.position.height as i64,
            },
        })
    }

    /// Convert one UI Automation element into an [`ElementInfo`].
    fn describe(element: &uiautomation::UIElement) -> Result<ElementInfo> {
        let name = element.get_name().unwrap_or_default();
        let role = element
            .get_control_type()
            .map(|control| format!("{control:?}"))
            .unwrap_or_else(|_| "unknown".to_owned());
        let bounds = element
            .get_bounding_rectangle()
            .ok()
            .map(|rect| BoundingBox {
                x: i64::from(rect.get_left()),
                y: i64::from(rect.get_top()),
                width: i64::from(rect.get_width()),
                height: i64::from(rect.get_height()),
            });
        let mut states = Vec::new();
        if element.is_enabled().unwrap_or(false) {
            states.push("enabled".to_owned());
        }
        if element.has_keyboard_focus().unwrap_or(false) {
            states.push("focused".to_owned());
        }
        if element.is_offscreen().unwrap_or(false) {
            states.push("offscreen".to_owned());
        }
        if element.is_password().unwrap_or(false) {
            states.push("password".to_owned());
        }
        let interactable =
            element.is_enabled().unwrap_or(false) && !element.is_offscreen().unwrap_or(true);
        Ok(ElementInfo {
            role,
            name,
            bounds,
            value: None,
            states,
            confidence: 0.95,
            interactable,
        })
    }

    /// Depth-capped breadth-first walk of the control-view tree.
    fn walk(
        automation: &uiautomation::UIAutomation,
        root: &uiautomation::UIElement,
        max_depth: usize,
    ) -> Result<ElementNode> {
        const MAX_NODES: usize = 400;
        let mut count = 0_usize;
        fn visit(
            automation: &uiautomation::UIAutomation,
            element: &uiautomation::UIElement,
            depth: usize,
            max_depth: usize,
            count: &mut usize,
        ) -> Result<ElementNode> {
            *count += 1;
            let info = WindowsDriver::describe(element)?;
            let mut children = Vec::new();
            if depth < max_depth && *count < MAX_NODES {
                if let Ok(condition) = automation.create_true_condition() {
                    if let Ok(found) =
                        element.find_all(uiautomation::types::TreeScope::Children, &condition)
                    {
                        for child in found.iter().take(64) {
                            match visit(automation, child, depth + 1, max_depth, count) {
                                Ok(node) => children.push(node),
                                Err(_) => continue,
                            }
                            if *count >= MAX_NODES {
                                break;
                            }
                        }
                    }
                }
            }
            Ok(ElementNode { info, children })
        }
        visit(automation, root, 0, max_depth.min(4), &mut count)
    }
}

impl AccessibilityDriver for WindowsDriver {
    fn backend_name(&self) -> &'static str {
        "win32-uia"
    }

    async fn focused_window(&self) -> Result<WindowInfo> {
        Self::active_window()
    }

    async fn element_tree(&self, _window: &WindowInfo, max_depth: usize) -> Result<ElementNode> {
        let automation = uiautomation::UIAutomation::new()
            .map_err(|error| err(&format!("uia init: {error}")))?;
        let root = automation
            .get_root_element()
            .map_err(|error| err(&format!("uia root: {error}")))?;
        Self::walk(&automation, &root, max_depth)
    }

    async fn element_at(&self, x: i64, y: i64) -> Result<ElementInfo> {
        let automation = uiautomation::UIAutomation::new()
            .map_err(|error| err(&format!("uia init: {error}")))?;
        let point = uiautomation::types::Point::new(
            i32::try_from(x).unwrap_or(i32::MAX),
            i32::try_from(y).unwrap_or(i32::MAX),
        );
        match automation.element_from_point(point) {
            Ok(element) => Self::describe(&element),
            Err(_) => Ok(ElementInfo {
                role: "point".to_owned(),
                name: format!("point {x},{y}"),
                bounds: Some(BoundingBox {
                    x,
                    y,
                    width: 1,
                    height: 1,
                }),
                value: None,
                states: Vec::new(),
                confidence: 0.5,
                interactable: true,
            }),
        }
    }

    async fn text_of(&self, element: &ElementInfo) -> Result<String> {
        if element.name.trim().is_empty() {
            return Err(err("element has no readable name"));
        }
        Ok(element.name.clone())
    }

    async fn focus_element(&self, _element: &ElementInfo) -> Result<()> {
        let automation = uiautomation::UIAutomation::new()
            .map_err(|error| err(&format!("uia init: {error}")))?;
        let focused = automation
            .get_focused_element()
            .map_err(|error| err(&format!("uia focus: {error}")))?;
        focused
            .set_focus()
            .map_err(|error| err(&format!("uia set_focus: {error}")))?;
        Ok(())
    }
}
