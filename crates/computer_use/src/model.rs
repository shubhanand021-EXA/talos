//! Computer-use model: actions, targets, receipts, zones, and safety config.
//!
//! Wire shapes mirror `@talos/contracts` `computer.ts` exactly (`camelCase`
//! JSON) so the TypeScript bridge validates every response with Zod.

/// Stable tool identifiers (`computer.*`).
pub const COMPUTER_MOVE_MOUSE: &str = "computer.move_mouse";
/// Single click.
pub const COMPUTER_CLICK: &str = "computer.click";
/// Double click.
pub const COMPUTER_DOUBLE_CLICK: &str = "computer.double_click";
/// Press-drag-release.
pub const COMPUTER_DRAG: &str = "computer.drag";
/// Type text.
pub const COMPUTER_TYPE_TEXT: &str = "computer.type_text";
/// Press a hotkey chord.
pub const COMPUTER_HOTKEY: &str = "computer.hotkey";
/// Scroll at a position.
pub const COMPUTER_SCROLL: &str = "computer.scroll";
/// Capture the virtual screen (read-only).
pub const COMPUTER_CAPTURE_SCREEN: &str = "computer.capture_screen";
/// Locate an element (read-only).
pub const COMPUTER_LOCATE_ELEMENT: &str = "computer.locate_element";

/// All computer-use tool identifiers in registration order.
pub const COMPUTER_TOOLS: &[&str] = &[
    COMPUTER_MOVE_MOUSE,
    COMPUTER_CLICK,
    COMPUTER_DOUBLE_CLICK,
    COMPUTER_DRAG,
    COMPUTER_TYPE_TEXT,
    COMPUTER_HOTKEY,
    COMPUTER_SCROLL,
    COMPUTER_CAPTURE_SCREEN,
    COMPUTER_LOCATE_ELEMENT,
];

/// All supported computer actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComputerAction {
    /// Move the pointer.
    MoveMouse,
    /// Single click.
    Click,
    /// Double click.
    DoubleClick,
    /// Press-drag-release.
    Drag,
    /// Type text.
    TypeText,
    /// Press a hotkey chord.
    Hotkey,
    /// Scroll.
    Scroll,
    /// Capture the screen (read-only).
    CaptureScreen,
    /// Locate an element (read-only).
    LocateElement,
}

/// Action target: coordinates, bounding box, application/window hints.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputerTarget {
    /// X coordinate in virtual-screen pixels.
    pub x: Option<i64>,
    /// Y coordinate in virtual-screen pixels.
    pub y: Option<i64>,
    /// Bounding-box width in pixels.
    pub width: Option<i64>,
    /// Bounding-box height in pixels.
    pub height: Option<i64>,
    /// Application hint (used for dangerous-zone matching).
    pub app_hint: Option<String>,
    /// Window title hint.
    pub window_hint: Option<String>,
    /// Minimum confidence override (defaults to the envelope threshold).
    pub confidence: Option<f64>,
}

/// Dangerous-zone categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DangerZoneKind {
    /// OS settings panels — always blocked.
    SystemSettings,
    /// Password managers and credential stores — always blocked.
    PasswordManager,
    /// Terminals and shells — warning, allowed with evidence.
    Terminal,
    /// Payment and checkout flows — warning, high-risk flagged.
    PaymentFlow,
}

/// One dangerous-zone rule.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DangerZone {
    /// Category.
    pub kind: DangerZoneKind,
    /// Substring matched (case-insensitive) against app/window hints.
    pub pattern: String,
    /// True for hard blocks; false for warnings.
    pub blocking: bool,
}

/// Safety envelope configuration.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SafetyConfig {
    /// Global master switch. Default false: nothing runs live until enabled.
    pub enabled: bool,
    /// Dry-run is the default mode for every action.
    pub dry_run_default: bool,
    /// Maximum live actions per rolling 60s window.
    pub max_actions_per_minute: u32,
    /// Virtual screen width in pixels.
    pub viewport_width: i64,
    /// Virtual screen height in pixels.
    pub viewport_height: i64,
    /// Minimum element confidence (0.0..=1.0).
    pub min_confidence: f64,
    /// Dangerous-zone rules.
    pub danger_zones: Vec<DangerZone>,
}

impl Default for SafetyConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            dry_run_default: true,
            max_actions_per_minute: 30,
            viewport_width: 1440,
            viewport_height: 900,
            min_confidence: 0.7,
            danger_zones: vec![
                DangerZone {
                    kind: DangerZoneKind::SystemSettings,
                    pattern: "settings".to_owned(),
                    blocking: true,
                },
                DangerZone {
                    kind: DangerZoneKind::SystemSettings,
                    pattern: "preferences".to_owned(),
                    blocking: true,
                },
                DangerZone {
                    kind: DangerZoneKind::PasswordManager,
                    pattern: "password".to_owned(),
                    blocking: true,
                },
                DangerZone {
                    kind: DangerZoneKind::PasswordManager,
                    pattern: "keychain".to_owned(),
                    blocking: true,
                },
                DangerZone {
                    kind: DangerZoneKind::PasswordManager,
                    pattern: "1password".to_owned(),
                    blocking: true,
                },
                DangerZone {
                    kind: DangerZoneKind::PasswordManager,
                    pattern: "bitwarden".to_owned(),
                    blocking: true,
                },
                DangerZone {
                    kind: DangerZoneKind::Terminal,
                    pattern: "terminal".to_owned(),
                    blocking: false,
                },
                DangerZone {
                    kind: DangerZoneKind::Terminal,
                    pattern: "console".to_owned(),
                    blocking: false,
                },
                DangerZone {
                    kind: DangerZoneKind::Terminal,
                    pattern: "shell".to_owned(),
                    blocking: false,
                },
                DangerZone {
                    kind: DangerZoneKind::PaymentFlow,
                    pattern: "payment".to_owned(),
                    blocking: true,
                },
                DangerZone {
                    kind: DangerZoneKind::PaymentFlow,
                    pattern: "checkout".to_owned(),
                    blocking: true,
                },
                DangerZone {
                    kind: DangerZoneKind::PaymentFlow,
                    pattern: "bank".to_owned(),
                    blocking: true,
                },
                DangerZone {
                    kind: DangerZoneKind::PaymentFlow,
                    pattern: "wallet".to_owned(),
                    blocking: true,
                },
            ],
        }
    }
}

/// A screen-space bounding box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoundingBox {
    /// Left edge in pixels.
    pub x: i64,
    /// Top edge in pixels.
    pub y: i64,
    /// Width in pixels.
    pub width: i64,
    /// Height in pixels.
    pub height: i64,
}

/// Auditable outcome of one computer action.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputerReceipt {
    /// Receipt identifier.
    pub id: String,
    /// Action identifier.
    pub action_id: String,
    /// Owning task.
    pub task_id: String,
    /// Action performed.
    pub action: ComputerAction,
    /// Outcome.
    pub status: String,
    /// True when nothing mutated.
    pub dry_run: bool,
    /// Deterministic screen hash (`sha256:`).
    pub screen_hash: Option<String>,
    /// Resolved bounding box, if any.
    pub bounding_box: Option<BoundingBox>,
    /// Element confidence (0.0..=1.0).
    pub confidence: Option<f64>,
    /// Human-readable detail.
    pub detail: String,
    /// Structured failure, if any.
    pub error: Option<SafetyViolation>,
    /// Policy verdict reference.
    pub policy_decision: String,
    /// Policy rationale.
    pub policy_reason: String,
    /// Evidence row holding this receipt, if persisted.
    pub evidence_id: Option<String>,
    /// Execution fidelity: dry-runs and non-executed actions are `mock`,
    /// real executions are `live`.
    pub execution_fidelity: talos_common::ExecutionFidelity,
    /// RFC 3339 execution timestamp.
    pub executed_at: String,
}

/// Machine-readable safety violation.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SafetyViolation {
    /// Stable code (`emergency_stop`, `computer_disabled`, `rate_limited`,
    /// `out_of_bounds`, `low_confidence`, `dangerous_zone`, ...).
    pub code: String,
    /// Human-readable message (safe to display).
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_ids_are_stable() {
        assert_eq!(
            COMPUTER_TOOLS,
            [
                "computer.move_mouse",
                "computer.click",
                "computer.double_click",
                "computer.drag",
                "computer.type_text",
                "computer.hotkey",
                "computer.scroll",
                "computer.capture_screen",
                "computer.locate_element",
            ]
        );
    }

    #[test]
    fn safety_defaults_are_conservative() {
        let config = SafetyConfig::default();
        assert!(!config.enabled);
        assert!(config.dry_run_default);
        assert!(config.max_actions_per_minute > 0);
        assert!((0.0..=1.0).contains(&config.min_confidence));
        assert!(config.danger_zones.iter().any(|zone| zone.blocking));
        assert!(config.danger_zones.iter().any(|zone| !zone.blocking));
    }

    #[test]
    fn actions_serialize_snake_case() {
        let raw = serde_json::to_string(&ComputerAction::DoubleClick).expect("serializes");
        assert_eq!(raw, "\"double_click\"");
    }
}
