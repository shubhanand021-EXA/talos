//! macOS accessibility driver: AXUIElement tree + active-window focus.
//!
//! Built on the safe `axuielement` bindings (no raw FFI in Talos code).
//! Requires Accessibility permission (System Settings → Privacy & Security);
//! without it every call fails closed with the AX trust error. All failures
//! degrade to coordinate mapping.

use talos_common::{Result, TalosError};

use crate::driver::{AccessibilityDriver, ElementInfo, ElementNode, WindowBounds, WindowInfo};
use crate::model::BoundingBox;

fn err(msg: &str) -> TalosError {
    TalosError::Storage(msg.to_owned())
}

/// AXUIElement backend for macOS (`macos-ax`).
#[derive(Debug, Default)]
pub struct MacDriver;

impl MacDriver {
    /// Create the driver (stateless).
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Focused window via the cross-platform active-window probe.
    fn active_window() -> Result<WindowInfo> {
        let active = active_win_pos_rs::get_active_window()
            .map_err(|error| err(&format!("active window probe failed: {error}")))?;
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

    /// Describe one AXUIElement (best-effort reads; missing attributes are
    /// empty, never fatal).
    fn describe(element: &axuielement::AXUIElement) -> ElementInfo {
        let role = element
            .string_attribute(axuielement::ax_attribute::AX_ROLE_ATTRIBUTE)
            .ok()
            .flatten()
            .unwrap_or_else(|| "unknown".to_owned());
        let name = element
            .string_attribute(axuielement::ax_attribute::AX_TITLE_ATTRIBUTE)
            .ok()
            .flatten()
            .or_else(|| {
                element
                    .string_attribute(axuielement::ax_attribute::AX_DESCRIPTION_ATTRIBUTE)
                    .ok()
                    .flatten()
            })
            .unwrap_or_default();
        let bounds = element
            .rect_attribute(axuielement::ax_attribute::AX_POSITION_ATTRIBUTE)
            .ok()
            .flatten()
            .and_then(|position| {
                element
                    .size_attribute(axuielement::ax_attribute::AX_SIZE_ATTRIBUTE)
                    .ok()
                    .flatten()
                    .map(|size| BoundingBox {
                        x: position.origin.x as i64,
                        y: position.origin.y as i64,
                        width: size.width as i64,
                        height: size.height as i64,
                    })
            });
        let mut states = Vec::new();
        if element
            .bool_attribute(axuielement::ax_attribute::AX_ENABLED_ATTRIBUTE)
            .ok()
            .flatten()
            .unwrap_or(false)
        {
            states.push("enabled".to_owned());
        }
        if element
            .bool_attribute(axuielement::ax_attribute::AX_FOCUSED_ATTRIBUTE)
            .ok()
            .flatten()
            .unwrap_or(false)
        {
            states.push("focused".to_owned());
        }
        ElementInfo {
            role,
            name,
            bounds,
            value: None,
            states,
            confidence: 0.9,
            interactable: true,
        }
    }

    /// Depth-capped recursion over `AXChildren`.
    fn walk(element: &axuielement::AXUIElement, depth: usize, max_depth: usize) -> ElementNode {
        const MAX_NODES: usize = 400;
        fn visit(
            element: &axuielement::AXUIElement,
            depth: usize,
            max_depth: usize,
            count: &mut usize,
        ) -> ElementNode {
            *count += 1;
            let info = MacDriver::describe(element);
            let mut children = Vec::new();
            if depth < max_depth && *count < MAX_NODES {
                if let Ok(kids) = element.children() {
                    for kid in kids.iter().take(64) {
                        children.push(visit(kid, depth + 1, max_depth, count));
                        if *count >= MAX_NODES {
                            break;
                        }
                    }
                }
            }
            ElementNode { info, children }
        }
        let mut count = 0_usize;
        visit(element, depth, max_depth.min(4), &mut count)
    }
}

impl AccessibilityDriver for MacDriver {
    fn backend_name(&self) -> &'static str {
        "macos-ax"
    }

    async fn focused_window(&self) -> Result<WindowInfo> {
        if !axuielement::is_process_trusted() {
            return Err(err(
                "macos accessibility permission missing: grant it in System Settings → Privacy & Security",
            ));
        }
        Self::active_window()
    }

    async fn element_tree(&self, _window: &WindowInfo, max_depth: usize) -> Result<ElementNode> {
        if !axuielement::is_process_trusted() {
            return Err(err("macos accessibility permission missing"));
        }
        let system = axuielement::system_wide().ok_or_else(|| err("no system-wide element"))?;
        let app = system
            .focused_application()
            .map_err(|error| err(&format!("focused application: {error}")))?
            .ok_or_else(|| err("no focused application"))?;
        Ok(Self::walk(&app, 0, max_depth))
    }

    async fn element_at(&self, x: i64, y: i64) -> Result<ElementInfo> {
        if !axuielement::is_process_trusted() {
            return Err(err("macos accessibility permission missing"));
        }
        let system =
            axuielement::AXUIElement::system_wide().ok_or_else(|| err("no system-wide element"))?;
        match system.element_at_position(x as f32, y as f32) {
            Ok(Some(element)) => Ok(Self::describe(&element)),
            _ => Ok(ElementInfo {
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
        let system = axuielement::system_wide().ok_or_else(|| err("no system-wide element"))?;
        let focused = system
            .focused_ui_element()
            .map_err(|error| err(&format!("focused element: {error}")))?
            .ok_or_else(|| err("no focused element"))?;
        focused
            .set_bool_attribute(true, axuielement::ax_attribute::AX_FOCUSED_ATTRIBUTE)
            .map_err(|error| err(&format!("ax focus: {error}")))?;
        Ok(())
    }
}
