//! Real OS driver implementation (feature-gated; never in CI).
//!
//! Observe → act → verify against the platform accessibility backend with
//! OS-level input injection. See [`crate::real`] for the gate contract.

use std::path::Path;
use std::sync::LazyLock;

use talos_common::{now_iso, Result, TalosError};

use crate::driver::{AccessibilityDriver, DriverAction, ElementInfo};
use crate::focus::{
    classify_move, manual_movement_detected, FocusEvent, FocusMonitor, KILL_SWITCH_TOLERANCE_PX,
};
use crate::injection::InjectionGuard;
use crate::platform::{action_center, PlatformDriver};
use crate::safety::SafetyHandle;
use crate::vision::{compare_frames, screenshot_from_png, verify_action, Screenshot};

fn err(msg: &str) -> TalosError {
    TalosError::Storage(msg.to_owned())
}

/// Process-wide injection state shared by the executor and the IPC layer.
static INJECTION: LazyLock<InjectionGuard> = LazyLock::new(InjectionGuard::new);

/// Whether the current process opted into real desktop control.
///
/// Requires `TALOS_COMPUTER_REAL=1` in the environment (on top of the cargo
/// feature, the enabled envelope, and a policy `Allow` verdict).
#[must_use]
pub fn real_mode_opted_in() -> bool {
    std::env::var("TALOS_COMPUTER_REAL")
        .map(|value| value == "1")
        .unwrap_or(false)
}

/// Latch the emergency stop process-wide and report the in-flight count.
///
/// Returns `(timestamp, in_flight)` for the activation log line.
pub fn emergency_release_all() -> (String, usize) {
    let in_flight = INJECTION.emergency_release();
    (now_iso(), in_flight)
}

/// Clear a latched stop (explicit human reset only).
pub fn emergency_reset_all() {
    INJECTION.reset();
}

/// Real adapter: accessibility driver + OS input + vision verification.
#[derive(Debug)]
pub struct RealComputer {
    envelope: SafetyHandle,
}

impl RealComputer {
    /// Build the adapter against a shared safety envelope.
    #[must_use]
    pub fn new(envelope: SafetyHandle) -> Self {
        Self { envelope }
    }

    /// Current pointer position, if the backend reports it.
    fn pointer_position() -> Result<(i64, i64)> {
        use enigo::{Enigo, Mouse, Settings};
        let enigo = Enigo::new(&Settings::default())
            .map_err(|error| err(&format!("input backend unavailable: {error}")))?;
        let (x, y) = enigo
            .location()
            .map_err(|error| err(&format!("pointer query failed: {error}")))?;
        Ok((i64::from(x), i64::from(y)))
    }

    /// Capture the primary monitor as a [`Screenshot`].
    fn capture() -> Result<Screenshot> {
        let monitors = xcap::Monitor::all()
            .map_err(|error| err(&format!("screen capture failed: {error}")))?;
        let monitor = monitors
            .into_iter()
            .next()
            .ok_or_else(|| err("no monitors found"))?;
        let image = monitor
            .capture_image()
            .map_err(|error| err(&format!("screen capture failed: {error}")))?;
        let (width, height) = (image.width(), image.height());
        let png = crate::vision::encode_png_rgba(width, height, &image.into_raw())
            .ok_or_else(|| err("png encode failed"))?;
        Ok(screenshot_from_png(png, width, height))
    }

    /// Inject one resolved action through the OS input path.
    ///
    /// Returns the expected pointer position afterwards, if the action moves
    /// the pointer. The [`InjectionGuard`] token is held for the whole
    /// injection so the emergency stop accounts it.
    fn inject(action: &DriverAction) -> Result<Option<(i64, i64)>> {
        use enigo::{Button, Coordinate, Direction, Enigo, Keyboard, Mouse, Settings};
        let _token = INJECTION.begin().map_err(|message| err(&message))?;
        let mut enigo = Enigo::new(&Settings::default())
            .map_err(|error| err(&format!("input backend unavailable: {error}")))?;
        let moved_to = match action {
            DriverAction::MoveTo { x, y } => {
                enigo
                    .move_mouse(*x as i32, *y as i32, Coordinate::Abs)
                    .map_err(|error| err(&format!("move failed: {error}")))?;
                Some((*x, *y))
            }
            DriverAction::Click { x, y, button } => {
                enigo
                    .move_mouse(*x as i32, *y as i32, Coordinate::Abs)
                    .map_err(|error| err(&format!("move failed: {error}")))?;
                let button = parse_button(button)?;
                enigo
                    .button(button, Direction::Click)
                    .map_err(|error| err(&format!("click failed: {error}")))?;
                Some((*x, *y))
            }
            DriverAction::DoubleClick { x, y } => {
                enigo
                    .move_mouse(*x as i32, *y as i32, Coordinate::Abs)
                    .map_err(|error| err(&format!("move failed: {error}")))?;
                for _ in 0..2 {
                    enigo
                        .button(Button::Left, Direction::Click)
                        .map_err(|error| err(&format!("double-click failed: {error}")))?;
                }
                Some((*x, *y))
            }
            DriverAction::Drag { x, y, x2, y2 } => {
                enigo
                    .move_mouse(*x as i32, *y as i32, Coordinate::Abs)
                    .map_err(|error| err(&format!("move failed: {error}")))?;
                enigo
                    .button(Button::Left, Direction::Press)
                    .map_err(|error| err(&format!("drag press failed: {error}")))?;
                enigo
                    .move_mouse(*x2 as i32, *y2 as i32, Coordinate::Abs)
                    .map_err(|error| err(&format!("drag move failed: {error}")))?;
                enigo
                    .button(Button::Left, Direction::Release)
                    .map_err(|error| err(&format!("drag release failed: {error}")))?;
                Some((*x2, *y2))
            }
            DriverAction::TypeText { text } => {
                enigo
                    .text(text)
                    .map_err(|error| err(&format!("type failed: {error}")))?;
                None
            }
            DriverAction::Hotkey { keys } => {
                press_chord(&mut enigo, keys)?;
                None
            }
            DriverAction::Scroll {
                x,
                y,
                direction,
                pixels,
            } => {
                if let (Some(x), Some(y)) = (x, y) {
                    enigo
                        .move_mouse(*x as i32, *y as i32, Coordinate::Abs)
                        .map_err(|error| err(&format!("move failed: {error}")))?;
                }
                let length = scroll_length(direction, *pixels)?;
                enigo
                    .scroll(length, scroll_axis(direction))
                    .map_err(|error| err(&format!("scroll failed: {error}")))?;
                None
            }
            DriverAction::Activate => None,
        };
        Ok(moved_to)
    }

    /// Run one tool call through observe → act → verify.
    ///
    /// Focus loss and manual pointer movement fail closed. Verification
    /// outcomes ride in the output (`verified`, `verificationFailed`,
    /// `focusLost`) for the event layer.
    #[allow(clippy::too_many_arguments)]
    pub async fn execute(
        &self,
        tool_id: &str,
        task_id: &str,
        params: &serde_json::Value,
        root: &Path,
        min_confidence: f64,
    ) -> Result<serde_json::Value> {
        let (action, center) = crate::real::plan_action(tool_id, params)?;
        let driver = PlatformDriver::system();
        let envelope = self.envelope.clone();
        let app_hint = params
            .get("appHint")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        let task_id = task_id.to_owned();
        let tool_id = tool_id.to_owned();
        let root = root.to_path_buf();
        tokio::task::spawn_blocking(move || {
            Self::execute_blocking(
                &driver,
                &envelope,
                &task_id,
                app_hint.as_deref(),
                &action,
                center,
                &tool_id,
                &root,
                min_confidence,
            )
        })
        .await
        .map_err(|error| err(&format!("real action panicked: {error}")))?
    }

    /// Blocking observe → act → verify body (runs on the blocking pool so OS
    /// calls never stall the async runtime).
    #[allow(clippy::too_many_arguments)]
    fn execute_blocking(
        driver: &PlatformDriver,
        envelope: &SafetyHandle,
        task_id: &str,
        app_hint: Option<&str>,
        action: &DriverAction,
        center: Option<(i64, i64)>,
        tool_id: &str,
        root: &Path,
        min_confidence: f64,
    ) -> Result<serde_json::Value> {
        // Observe (1): frame + focus before acting.
        let before = Self::capture()?;
        let focused_before = block_driver(driver.focused_window())?;
        let scope_app = app_hint
            .map(str::to_owned)
            .unwrap_or_else(|| focused_before.app_name.clone());
        // Session isolation + budgets (counted once per live action).
        block_on_fresh(envelope.check_session(task_id, Some(&scope_app))).map_err(
            |violation: crate::model::SafetyViolation| {
                err(&format!("{}: {}", violation.code, violation.message))
            },
        )?;
        // Resolve target element (accessibility hit-test, then fallback).
        let element = Self::resolve_target(driver, center)?;
        if element.confidence < min_confidence {
            return Err(err(&format!(
                "low confidence {:.2} < {:.2} for {tool_id}",
                element.confidence, min_confidence
            )));
        }
        // Focus the target window best-effort; injection verifies anyway.
        let _ = block_driver(driver.focus_element(&element));
        // Kill-switch baseline, then inject.
        let pointer_before = Self::pointer_position().ok();
        Self::inject(action)?;
        let pointer_after = Self::pointer_position().ok();
        match (
            action_center(action, &element),
            pointer_before,
            pointer_after,
        ) {
            (Some(expected), _, Some(actual))
                if manual_movement_detected(expected, actual, KILL_SWITCH_TOLERANCE_PX) =>
            {
                return Err(err(
                    "manual pointer movement detected: injection aborted (human took control)",
                ));
            }
            (None, Some(before_pos), Some(after_pos))
                if manual_movement_detected(before_pos, after_pos, KILL_SWITCH_TOLERANCE_PX) =>
            {
                return Err(err(
                    "manual pointer movement detected during input (human took control)",
                ));
            }
            _ => {}
        }
        // Observe (2): focus must not have escaped to a blocked app or nowhere.
        let mut focus_lost = false;
        if let Ok(focused_after) = block_driver(driver.focused_window()) {
            let monitor = FocusMonitor::expecting(focused_before.clone());
            match monitor.observe(Some(&focused_after)) {
                FocusEvent::Steady => {}
                FocusEvent::Moved { to, .. } => {
                    // Window switches can be legitimate (the click may have
                    // opened one) unless the new app is session-blocked.
                    let scope = block_on_fresh(envelope.session_scope());
                    if !matches!(classify_move(&scope, task_id, &to), FocusEvent::Steady) {
                        focus_lost = true;
                    }
                }
                FocusEvent::FocusLost { .. } | FocusEvent::UnexpectedApp { .. } => {
                    focus_lost = true;
                }
            }
        }
        let after = Self::capture()?;
        // Verify: frame comparison against the targeted region (click-like
        // actions must change something; reads never fail verification).
        let comparison = compare_frames(&before, &after);
        let expected_region = match action {
            DriverAction::Click { .. }
            | DriverAction::DoubleClick { .. }
            | DriverAction::Drag { .. } => element.bounds.as_ref(),
            _ => None,
        };
        let verdict = verify_action(&comparison, expected_region, tool_id);
        // Evidence: content-addressed artifacts + thumbnail.
        let before_artifact = crate::artifacts::store_screenshot(root, &before.png)?;
        let after_artifact = crate::artifacts::store_screenshot(root, &after.png)?;
        let thumbnail = thumbnail_data_uri(&after).unwrap_or_default();
        // Budget accounting.
        block_on_fresh(envelope.note_session_action(task_id)).map_err(
            |violation: crate::model::SafetyViolation| {
                err(&format!("{}: {}", violation.code, violation.message))
            },
        )?;
        let viewport_width = envelope.config().viewport_width;
        let viewport_height = envelope.config().viewport_height;
        Ok(serde_json::json!({
            "simulated": false,
            "backend": driver.backend_name(),
            "screenHash": after.hash,
            "beforeHash": before.hash,
            "afterHash": after.hash,
            "beforeArtifact": before_artifact.path.to_string_lossy(),
            "afterArtifact": after_artifact.path.to_string_lossy(),
            "verified": !verdict.verification_failed && !focus_lost,
            "verificationFailed": verdict.verification_failed,
            "verificationDetail": verdict.detail,
            "focusLost": focus_lost,
            "boundingBox": element.bounds,
            "confidence": element.confidence,
            "element": { "role": element.role, "name": element.name },
            "thumbnail": thumbnail,
            "viewport": { "width": viewport_width, "height": viewport_height },
        }))
    }

    /// Resolve the action's target element (hit-test or focused fallback).
    fn resolve_target(driver: &PlatformDriver, center: Option<(i64, i64)>) -> Result<ElementInfo> {
        match center {
            Some((x, y)) => block_driver(driver.element_at(x, y)),
            None => Ok(ElementInfo {
                role: "focused".to_owned(),
                name: "focused element".to_owned(),
                bounds: None,
                value: None,
                states: Vec::new(),
                confidence: 0.9,
                interactable: true,
            }),
        }
    }
}

impl crate::real::RealComputerAdapter for RealComputer {
    fn backend_name(&self) -> &'static str {
        "real-os-control"
    }

    fn capabilities(&self) -> Result<crate::real::RealCapabilities> {
        Ok(crate::real::RealCapabilities {
            backend: "real-os-control".to_owned(),
            pointer: true,
            keyboard: true,
            capture: true,
        })
    }

    fn move_pointer(&self, x: i64, y: i64) -> Result<()> {
        use enigo::{Coordinate, Enigo, Mouse, Settings};
        let _token = INJECTION.begin().map_err(|message| err(&message))?;
        let mut enigo = Enigo::new(&Settings::default())
            .map_err(|error| err(&format!("input backend unavailable: {error}")))?;
        enigo
            .move_mouse(x as i32, y as i32, Coordinate::Abs)
            .map_err(|error| err(&format!("move failed: {error}")))?;
        Ok(())
    }
}

/// Route one live computer tool through the real driver when opted in.
///
/// Returns `None` when real mode is off (the caller falls through to the
/// mock); `Some(Err)` on real-mode failures. Read-only tools capture real
/// frames/metadata without injecting.
pub async fn try_execute_live(
    tool_id: &str,
    task_id: &str,
    params: &serde_json::Value,
    root: &Path,
    envelope: &SafetyHandle,
) -> Option<Result<serde_json::Value>> {
    if !real_mode_opted_in() {
        return None;
    }
    if tool_id == "computer.capture_screen" || tool_id == "computer.locate_element" {
        return Some(read_only_live(tool_id, params, root).await);
    }
    let computer = RealComputer::new(envelope.clone());
    let min_confidence = envelope.config().min_confidence;
    Some(
        computer
            .execute(tool_id, task_id, params, root, min_confidence)
            .await,
    )
}

/// Read-only live path: real screenshot / element metadata, no injection.
async fn read_only_live(
    tool_id: &str,
    params: &serde_json::Value,
    root: &Path,
) -> Result<serde_json::Value> {
    let driver = PlatformDriver::system();
    let frame = tokio::task::spawn_blocking(RealComputer::capture)
        .await
        .map_err(|error| err(&format!("capture task failed: {error}")))??;
    let artifact = crate::artifacts::store_screenshot(root, &frame.png)?;
    let thumbnail = thumbnail_data_uri(&frame).unwrap_or_default();
    if tool_id == "computer.capture_screen" {
        return Ok(serde_json::json!({
            "simulated": false,
            "backend": driver.backend_name(),
            "viewport": { "width": frame.width, "height": frame.height },
            "screenHash": frame.hash,
            "artifact": artifact.path.to_string_lossy(),
            "thumbnail": thumbnail,
        }));
    }
    let selector = params
        .get("selector")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    // Best-effort metadata: real drivers resolve by point; selector-only
    // locate degrades to the coordinate fallback with a low confidence
    // marker so the envelope gate still applies downstream.
    let element = ElementInfo {
        role: "query".to_owned(),
        name: selector.to_owned(),
        bounds: None,
        value: None,
        states: Vec::new(),
        confidence: 0.5,
        interactable: false,
    };
    Ok(serde_json::json!({
        "simulated": false,
        "backend": driver.backend_name(),
        "selector": selector,
        "resolved": { "selector": selector, "found": false, "confidence": 0.5 },
        "boundingBox": element.bounds,
        "screenHash": frame.hash,
        "artifact": artifact.path.to_string_lossy(),
        "thumbnail": thumbnail,
    }))
}

/// Run a driver future to completion from blocking code.
///
/// Builds a fresh single-thread runtime per call: `execute_blocking` runs on
/// Tokio's blocking pool, where driving a future on the ambient runtime
/// would be unsound, so each driver call gets its own executor.
fn block_driver<T>(future: impl std::future::Future<Output = T>) -> T {
    block_on_fresh(future)
}

/// Drive any future to completion from blocking code (fresh runtime).
fn block_on_fresh<F, T>(future: F) -> T
where
    F: std::future::Future<Output = T>,
{
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("driver runtime builds")
        .block_on(future)
}

/// Validate a mouse button name.
fn parse_button(name: &str) -> Result<enigo::Button> {
    match name.to_lowercase().as_str() {
        "left" => Ok(enigo::Button::Left),
        "right" => Ok(enigo::Button::Right),
        "middle" => Ok(enigo::Button::Middle),
        other => Err(err(&format!("unsupported mouse button: {other}"))),
    }
}

/// Press a `+`-separated hotkey chord, releasing in reverse order.
fn press_chord(enigo: &mut enigo::Enigo, keys: &str) -> Result<()> {
    use enigo::{Direction, Keyboard};
    let mut pressed = Vec::new();
    for token in keys.split('+') {
        let key = parse_key(token.trim())?;
        enigo
            .key(key, Direction::Press)
            .map_err(|error| err(&format!("hotkey press failed: {error}")))?;
        pressed.push(key);
    }
    for key in pressed.iter().rev() {
        enigo
            .key(*key, Direction::Release)
            .map_err(|error| err(&format!("hotkey release failed: {error}")))?;
    }
    Ok(())
}

/// Map one chord token to an enigo key (fail closed on unknown tokens).
fn parse_key(token: &str) -> Result<enigo::Key> {
    use enigo::Key;
    let lower = token.to_lowercase();
    match lower.as_str() {
        "ctrl" | "control" => Ok(Key::Control),
        "shift" => Ok(Key::Shift),
        "alt" => Ok(Key::Alt),
        "meta" | "cmd" | "command" | "win" | "windows" | "super" => Ok(Key::Meta),
        "enter" | "return" => Ok(Key::Return),
        "tab" => Ok(Key::Tab),
        "esc" | "escape" => Ok(Key::Escape),
        "del" | "delete" => Ok(Key::Delete),
        "backspace" => Ok(Key::Backspace),
        "space" | " " => Ok(Key::Space),
        "up" | "uparrow" => Ok(Key::UpArrow),
        "down" | "downarrow" => Ok(Key::DownArrow),
        "left" | "leftarrow" => Ok(Key::LeftArrow),
        "right" | "rightarrow" => Ok(Key::RightArrow),
        "home" => Ok(Key::Home),
        "end" => Ok(Key::End),
        "pageup" => Ok(Key::PageUp),
        "pagedown" => Ok(Key::PageDown),
        token if token.starts_with('f') => {
            let number: u8 = token[1..]
                .parse()
                .map_err(|_| err(&format!("unsupported hotkey token: {token}")))?;
            match number {
                1 => Ok(Key::F1),
                2 => Ok(Key::F2),
                3 => Ok(Key::F3),
                4 => Ok(Key::F4),
                5 => Ok(Key::F5),
                6 => Ok(Key::F6),
                7 => Ok(Key::F7),
                8 => Ok(Key::F8),
                9 => Ok(Key::F9),
                10 => Ok(Key::F10),
                11 => Ok(Key::F11),
                12 => Ok(Key::F12),
                13 => Ok(Key::F13),
                14 => Ok(Key::F14),
                15 => Ok(Key::F15),
                16 => Ok(Key::F16),
                17 => Ok(Key::F17),
                18 => Ok(Key::F18),
                19 => Ok(Key::F19),
                20 => Ok(Key::F20),
                _ => Err(err(&format!("unsupported hotkey token: {token}"))),
            }
        }
        token => {
            let mut chars = token.chars();
            match (chars.next(), chars.next()) {
                (Some(single), None) => Ok(Key::Unicode(single)),
                _ => Err(err(&format!("unsupported hotkey token: {token}"))),
            }
        }
    }
}

/// Scroll length + sign for a direction.
fn scroll_length(direction: &str, pixels: i64) -> Result<i32> {
    let magnitude = pixels.clamp(1, 10_000) as i32;
    match direction.to_lowercase().as_str() {
        "up" | "left" => Ok(magnitude),
        "down" | "right" => Ok(-magnitude),
        other => Err(err(&format!("unsupported scroll direction: {other}"))),
    }
}

/// Scroll axis for a direction.
fn scroll_axis(direction: &str) -> enigo::Axis {
    match direction.to_lowercase().as_str() {
        "left" | "right" => enigo::Axis::Horizontal,
        _ => enigo::Axis::Vertical,
    }
}

/// Downscale a frame to a thumbnail data URI (max 480px wide).
fn thumbnail_data_uri(frame: &Screenshot) -> Result<String> {
    let image = image::load_from_memory(&frame.png)
        .map_err(|error| err(&format!("thumbnail decode: {error}")))?;
    let (width, height) = (image.width(), image.height());
    let scale = 480.0 / width.max(1) as f32;
    let target_width = 480.min(width);
    let target_height = ((height as f32 * scale).round() as u32).max(1);
    let small = image.resize(
        target_width,
        target_height,
        image::imageops::FilterType::Triangle,
    );
    let rgba = small.to_rgba8();
    let png = crate::vision::encode_png_rgba(rgba.width(), rgba.height(), rgba.as_raw())
        .ok_or_else(|| err("thumbnail encode failed"))?;
    Ok(format!(
        "data:image/png;base64,{}",
        crate::vision::base64_encode(&png)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_mode_requires_explicit_opt_in() {
        std::env::remove_var("TALOS_COMPUTER_REAL");
        assert!(!real_mode_opted_in());
        std::env::set_var("TALOS_COMPUTER_REAL", "0");
        assert!(!real_mode_opted_in());
        std::env::set_var("TALOS_COMPUTER_REAL", "1");
        assert!(real_mode_opted_in());
        std::env::remove_var("TALOS_COMPUTER_REAL");
    }

    #[test]
    fn hotkey_chords_parse_deterministically() {
        assert!(parse_key("ctrl").is_ok());
        assert!(parse_key("s").is_ok());
        assert!(parse_key("F12").is_ok());
        assert!(parse_key("F21").is_err());
        assert!(parse_key("hyper").is_err());
        assert!(parse_key("").is_err());
    }

    #[test]
    fn scroll_directions_map_signs() {
        assert!(scroll_length("up", 100).expect("up") > 0);
        assert!(scroll_length("down", 100).expect("down") < 0);
        assert!(scroll_length("sideways", 100).is_err());
    }
}
