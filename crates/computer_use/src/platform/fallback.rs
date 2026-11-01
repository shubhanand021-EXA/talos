//! Coordinate-mapping fallback: no accessibility tree available.
//!
//! The fallback resolves every point as itself (confidence 0.5) and reports
//! focus from the cross-platform active-window probe. It keeps the
//! observe → act → verify loop total on platforms without a native driver:
//! actions still verify through before/after frame comparison.

use talos_common::{Result, TalosError};

use crate::driver::{AccessibilityDriver, ElementInfo, ElementNode, WindowBounds, WindowInfo};
use crate::model::BoundingBox;

fn err(msg: &str) -> TalosError {
    TalosError::Storage(msg.to_owned())
}

/// Coordinate-mapping fallback backend (`fallback`).
#[derive(Debug, Default)]
pub struct FallbackDriver;

impl FallbackDriver {
    /// Create the fallback driver.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Point element with coordinate-mapping confidence.
    fn point_element(x: i64, y: i64) -> ElementInfo {
        ElementInfo {
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
        }
    }
}

impl AccessibilityDriver for FallbackDriver {
    fn backend_name(&self) -> &'static str {
        "fallback"
    }

    async fn focused_window(&self) -> Result<WindowInfo> {
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

    async fn element_tree(&self, _window: &WindowInfo, _max_depth: usize) -> Result<ElementNode> {
        Err(err(
            "no accessibility tree on this platform: coordinate mapping only",
        ))
    }

    async fn element_at(&self, x: i64, y: i64) -> Result<ElementInfo> {
        Ok(Self::point_element(x, y))
    }

    async fn text_of(&self, element: &ElementInfo) -> Result<String> {
        if element.name.trim().is_empty() {
            return Err(err("element has no readable name"));
        }
        Ok(element.name.clone())
    }

    async fn focus_element(&self, _element: &ElementInfo) -> Result<()> {
        Err(err(
            "no programmatic focus on this platform: injection verifies focus instead",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fallback_resolves_points_without_a_tree() {
        let driver = FallbackDriver::new();
        assert_eq!(driver.backend_name(), "fallback");
        let element = driver.element_at(10, 20).await.expect("point resolves");
        assert_eq!(element.confidence, 0.5);
        assert!(driver
            .element_tree(
                &WindowInfo {
                    title: "w".to_owned(),
                    app_name: "a".to_owned(),
                    process_id: None,
                    bounds: WindowBounds {
                        x: 0,
                        y: 0,
                        width: 1,
                        height: 1
                    },
                },
                2
            )
            .await
            .is_err());
    }
}
