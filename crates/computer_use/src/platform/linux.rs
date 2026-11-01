//! Linux accessibility driver: AT-SPI2 tree via D-Bus + active-window focus.
//!
//! Built on the `atspi` crate (registry connection, accessible proxies,
//! component extents). Requires a running at-spi bus (present on GNOME/KDE
//! desktops). Every D-Bus failure degrades to coordinate mapping.

use talos_common::{Result, TalosError};

use crate::driver::{AccessibilityDriver, ElementInfo, ElementNode, WindowBounds, WindowInfo};
use crate::model::BoundingBox;

fn err(msg: &str) -> TalosError {
    TalosError::Storage(msg.to_owned())
}

/// AT-SPI2 backend for Linux (`linux-atspi`).
#[derive(Debug, Default)]
pub struct LinuxDriver;

impl LinuxDriver {
    /// Create the driver (stateless; connections are built per call).
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

    /// Open the registry connection (fails closed without an at-spi bus).
    async fn connect() -> Result<atspi::AccessibilityConnection> {
        atspi::AccessibilityConnection::new()
            .await
            .map_err(|error| err(&format!("at-spi bus unavailable: {error}")))
    }

    /// Build an accessible proxy for one object reference.
    async fn proxy_for(
        conn: &atspi::AccessibilityConnection,
        object: &atspi::ObjectRefOwned,
    ) -> Result<atspi::proxy::accessible::AccessibleProxy<'_>> {
        use atspi::connection::P2P;
        conn.object_as_accessible(object)
            .await
            .map_err(|error| err(&format!("accessible proxy: {error}")))
    }

    /// Describe one accessible object (best-effort reads).
    async fn describe(proxy: &atspi::proxy::accessible::AccessibleProxy<'_>) -> ElementInfo {
        let name = proxy.name().await.unwrap_or_default();
        let role = match proxy.get_role().await {
            Ok(role) => format!("{role:?}"),
            Err(_) => "unknown".to_owned(),
        };
        let bounds = Self::extents_of(proxy).await;
        // Interactable means "actionable at minimum via coordinates": the
        // coordinate-mapping fallback always applies, so resolution success
        // is the gate (confidence carries the uncertainty downstream).
        ElementInfo {
            role,
            name,
            bounds,
            value: None,
            states: Vec::new(),
            confidence: 0.9,
            interactable: true,
        }
    }

    /// Screen extents via the Component interface (None when unsupported).
    async fn extents_of(
        proxy: &atspi::proxy::accessible::AccessibleProxy<'_>,
    ) -> Option<BoundingBox> {
        let inner = proxy.inner().clone();
        let component = atspi::proxy::component::ComponentProxy::from(inner);
        let (x, y, width, height) = component.get_extents(atspi::CoordType::Screen).await.ok()?;
        if width <= 0 || height <= 0 {
            return None;
        }
        Some(BoundingBox {
            x: x as i64,
            y: y as i64,
            width: width as i64,
            height: height as i64,
        })
    }

    /// Depth-capped recursion over accessible children.
    async fn walk(
        conn: &atspi::AccessibilityConnection,
        object: &atspi::ObjectRefOwned,
        depth: usize,
        max_depth: usize,
        count: &mut usize,
    ) -> Result<ElementNode> {
        const MAX_NODES: usize = 400;
        *count += 1;
        let proxy = Self::proxy_for(conn, object).await?;
        let info = Self::describe(&proxy).await;
        let mut children = Vec::new();
        if depth < max_depth && *count < MAX_NODES {
            if let Ok(kids) = proxy.get_children().await {
                for kid in kids.iter().take(64) {
                    match Box::pin(Self::walk(conn, kid, depth + 1, max_depth, count)).await {
                        Ok(node) => children.push(node),
                        Err(_) => continue,
                    }
                    if *count >= MAX_NODES {
                        break;
                    }
                }
            }
        }
        Ok(ElementNode { info, children })
    }
}

impl AccessibilityDriver for LinuxDriver {
    fn backend_name(&self) -> &'static str {
        "linux-atspi"
    }

    async fn focused_window(&self) -> Result<WindowInfo> {
        Self::active_window()
    }

    async fn element_tree(&self, _window: &WindowInfo, max_depth: usize) -> Result<ElementNode> {
        let conn = Self::connect().await?;
        let root = conn
            .root_accessible_on_registry()
            .await
            .map_err(|error| err(&format!("at-spi root: {error}")))?;
        let root_ref: atspi::ObjectRefOwned = (&root)
            .try_into()
            .map_err(|error| err(&format!("at-spi root ref: {error}")))?;
        let mut count = 0_usize;
        Self::walk(&conn, &root_ref, 0, max_depth.min(4), &mut count).await
    }

    async fn element_at(&self, x: i64, y: i64) -> Result<ElementInfo> {
        let conn = Self::connect().await?;
        let root = conn
            .root_accessible_on_registry()
            .await
            .map_err(|error| err(&format!("at-spi root: {error}")))?;
        let inner = root.inner().clone();
        let component = atspi::proxy::component::ComponentProxy::from(inner);
        let hit = component
            .get_accessible_at_point(x as i32, y as i32, atspi::CoordType::Screen)
            .await
            .map_err(|error| err(&format!("at-spi hit test: {error}")))?;
        let proxy = Self::proxy_for(&conn, &hit).await?;
        Ok(Self::describe(&proxy).await)
    }

    async fn text_of(&self, element: &ElementInfo) -> Result<String> {
        if element.name.trim().is_empty() {
            return Err(err("element has no readable name"));
        }
        Ok(element.name.clone())
    }

    async fn focus_element(&self, _element: &ElementInfo) -> Result<()> {
        Err(err(
            "at-spi focus activation is not implemented: injection verifies focus instead",
        ))
    }
}
