//! Platform accessibility drivers: one real backend per OS.
//!
//! The dispatcher exposes a single [`PlatformDriver`] enum; each variant is
//! compiled only on its platform **and** only with the `real-os-control`
//! cargo feature. Unsupported combinations resolve to [`FallbackDriver`],
//! which reports structured "unsupported" errors so callers degrade to
//! coordinate mapping. No driver runs without the feature gate, the policy
//! verdict, the enabled envelope, and (at runtime) `TALOS_COMPUTER_REAL=1`.

use crate::driver::{AccessibilityDriver, DriverAction, ElementInfo, ElementNode, WindowInfo};

#[cfg(feature = "real-os-control")]
pub mod fallback;
#[cfg(all(feature = "real-os-control", target_os = "linux"))]
pub mod linux;
#[cfg(all(feature = "real-os-control", target_os = "macos"))]
pub mod macos;
#[cfg(all(feature = "real-os-control", target_os = "windows"))]
pub mod windows;

/// Real platform driver selected at compile time.
#[cfg(feature = "real-os-control")]
#[derive(Debug)]
pub enum PlatformDriver {
    /// Windows UI Automation backend.
    #[cfg(target_os = "windows")]
    Windows(windows::WindowsDriver),
    /// macOS AXUIElement backend.
    #[cfg(target_os = "macos")]
    Macos(macos::MacDriver),
    /// Linux AT-SPI2 backend.
    #[cfg(target_os = "linux")]
    Linux(linux::LinuxDriver),
    /// Coordinate-mapping fallback (no accessibility tree).
    Fallback(fallback::FallbackDriver),
}

#[cfg(feature = "real-os-control")]
impl PlatformDriver {
    /// Build the best driver for this platform.
    #[must_use]
    pub fn system() -> Self {
        #[cfg(target_os = "windows")]
        return Self::Windows(windows::WindowsDriver::new());
        #[cfg(target_os = "macos")]
        return Self::Macos(macos::MacDriver::new());
        #[cfg(target_os = "linux")]
        return Self::Linux(linux::LinuxDriver::new());
        #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
        return Self::Fallback(fallback::FallbackDriver::new());
    }
}

#[cfg(feature = "real-os-control")]
impl AccessibilityDriver for PlatformDriver {
    fn backend_name(&self) -> &'static str {
        match self {
            #[cfg(target_os = "windows")]
            Self::Windows(driver) => driver.backend_name(),
            #[cfg(target_os = "macos")]
            Self::Macos(driver) => driver.backend_name(),
            #[cfg(target_os = "linux")]
            Self::Linux(driver) => driver.backend_name(),
            Self::Fallback(driver) => driver.backend_name(),
        }
    }

    async fn focused_window(&self) -> talos_common::Result<WindowInfo> {
        match self {
            #[cfg(target_os = "windows")]
            Self::Windows(driver) => driver.focused_window().await,
            #[cfg(target_os = "macos")]
            Self::Macos(driver) => driver.focused_window().await,
            #[cfg(target_os = "linux")]
            Self::Linux(driver) => driver.focused_window().await,
            Self::Fallback(driver) => driver.focused_window().await,
        }
    }

    async fn element_tree(
        &self,
        window: &WindowInfo,
        max_depth: usize,
    ) -> talos_common::Result<ElementNode> {
        match self {
            #[cfg(target_os = "windows")]
            Self::Windows(driver) => driver.element_tree(window, max_depth).await,
            #[cfg(target_os = "macos")]
            Self::Macos(driver) => driver.element_tree(window, max_depth).await,
            #[cfg(target_os = "linux")]
            Self::Linux(driver) => driver.element_tree(window, max_depth).await,
            Self::Fallback(driver) => driver.element_tree(window, max_depth).await,
        }
    }

    async fn element_at(&self, x: i64, y: i64) -> talos_common::Result<ElementInfo> {
        match self {
            #[cfg(target_os = "windows")]
            Self::Windows(driver) => driver.element_at(x, y).await,
            #[cfg(target_os = "macos")]
            Self::Macos(driver) => driver.element_at(x, y).await,
            #[cfg(target_os = "linux")]
            Self::Linux(driver) => driver.element_at(x, y).await,
            Self::Fallback(driver) => driver.element_at(x, y).await,
        }
    }

    async fn text_of(&self, element: &ElementInfo) -> talos_common::Result<String> {
        match self {
            #[cfg(target_os = "windows")]
            Self::Windows(driver) => driver.text_of(element).await,
            #[cfg(target_os = "macos")]
            Self::Macos(driver) => driver.text_of(element).await,
            #[cfg(target_os = "linux")]
            Self::Linux(driver) => driver.text_of(element).await,
            Self::Fallback(driver) => driver.text_of(element).await,
        }
    }

    async fn focus_element(&self, element: &ElementInfo) -> talos_common::Result<()> {
        match self {
            #[cfg(target_os = "windows")]
            Self::Windows(driver) => driver.focus_element(element).await,
            #[cfg(target_os = "macos")]
            Self::Macos(driver) => driver.focus_element(element).await,
            #[cfg(target_os = "linux")]
            Self::Linux(driver) => driver.focus_element(element).await,
            Self::Fallback(driver) => driver.focus_element(element).await,
        }
    }
}

/// Map a resolved action to its target center for coordinate injection.
#[must_use]
pub fn action_center(action: &DriverAction, element: &ElementInfo) -> Option<(i64, i64)> {
    match *action {
        DriverAction::MoveTo { x, y }
        | DriverAction::Click { x, y, .. }
        | DriverAction::DoubleClick { x, y } => Some((x, y)),
        DriverAction::Drag { x, y, .. } => Some((x, y)),
        DriverAction::Scroll { x, y, .. } => match (x, y) {
            (Some(x), Some(y)) => Some((x, y)),
            _ => element_center(element),
        },
        DriverAction::TypeText { .. } | DriverAction::Hotkey { .. } => element_center(element),
        DriverAction::Activate => element_center(element),
    }
}

/// Center of an element's bounding box, if known.
#[must_use]
pub fn element_center(element: &ElementInfo) -> Option<(i64, i64)> {
    element
        .bounds
        .map(|bbox| (bbox.x + bbox.width / 2, bbox.y + bbox.height / 2))
}
