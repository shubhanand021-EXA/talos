//! Deterministic mock computer: a virtual 1440x900 screen with seeded
//! elements, resolution reports, and simulated action receipts.
//!
//! No OS calls, no display, no network. Live execution here means "the mock
//! ran" (`simulated: true`) — real OS control is explicitly not implemented
//! (see [`crate::real`]).

use sha2::{Digest, Sha256};
use talos_common::{Result, TalosError};

use crate::model::BoundingBox;

fn err(msg: &str) -> TalosError {
    TalosError::Storage(msg.to_owned())
}

fn hash_hex(input: &str) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(input.as_bytes())))
}

/// Tiny deterministic FNV-1a hash (stable across runs and platforms).
fn simple_hash(input: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in input.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// One deterministic virtual-screen element.
#[derive(Debug, Clone)]
pub struct MockElement {
    /// Element role.
    pub role: &'static str,
    /// Element label.
    pub label: &'static str,
    /// Bounding box in virtual-screen pixels.
    pub bbox: BoundingBox,
    /// Whether the element is visible.
    pub visible: bool,
    /// Whether the element accepts input.
    pub interactable: bool,
}

/// Fixed virtual-screen furniture plus a URL-seeded control.
fn mock_elements(url: Option<&str>) -> Vec<MockElement> {
    let mut elements = vec![
        MockElement {
            role: "taskbar",
            label: "taskbar",
            bbox: BoundingBox {
                x: 0,
                y: 860,
                width: 1440,
                height: 40,
            },
            visible: true,
            interactable: false,
        },
        MockElement {
            role: "window",
            label: "main-window",
            bbox: BoundingBox {
                x: 40,
                y: 40,
                width: 1360,
                height: 800,
            },
            visible: true,
            interactable: false,
        },
        MockElement {
            role: "button",
            label: "button#ok",
            bbox: BoundingBox {
                x: 640,
                y: 480,
                width: 160,
                height: 48,
            },
            visible: true,
            interactable: true,
        },
        MockElement {
            role: "textbox",
            label: "input#q",
            bbox: BoundingBox {
                x: 560,
                y: 400,
                width: 320,
                height: 40,
            },
            visible: true,
            interactable: true,
        },
    ];
    if let Some(context) = url {
        let seed = simple_hash(context);
        elements.push(MockElement {
            role: "link",
            label: "page-link",
            bbox: BoundingBox {
                x: (seed % 1100) as i64 + 20,
                y: (seed % 700) as i64 + 60,
                width: 120,
                height: 32,
            },
            visible: true,
            interactable: true,
        });
    }
    elements
}

/// Resolution report for a selector against the virtual screen.
#[derive(Debug, Clone)]
pub struct ResolvedElement {
    /// Selector that was resolved.
    pub selector: String,
    /// Whether an element matched.
    pub found: bool,
    /// Whether it is visible.
    pub visible: bool,
    /// Whether it accepts input.
    pub interactable: bool,
    /// Deterministic confidence (0.42 for `uncertain`, else 0.5..=0.99).
    pub confidence: f64,
    /// Bounding box of the match (or the viewport for misses).
    pub bbox: BoundingBox,
}

/// Resolve a selector deterministically.
///
/// Selectors containing `missing` resolve as absent; containing `hidden`
/// resolve as present-but-not-visible; containing `disabled` resolve as
/// present-but-not-interactable; containing `uncertain` yield low confidence.
pub fn resolve_selector(selector: &str) -> ResolvedElement {
    let found = !selector.contains("missing");
    let visible = found && !selector.contains("hidden");
    let interactable = visible && !selector.contains("disabled");
    let confidence = if !found {
        0.0
    } else if selector.contains("uncertain") {
        0.42
    } else {
        0.5 + (simple_hash(selector) % 50) as f64 / 100.0
    };
    let bbox = if found {
        let hash = simple_hash(selector);
        BoundingBox {
            x: (hash % 1100) as i64 + 20,
            y: (hash % 700) as i64 + 60,
            width: 80 + (hash % 200) as i64,
            height: 24 + (hash % 60) as i64,
        }
    } else {
        BoundingBox {
            x: 0,
            y: 0,
            width: 1440,
            height: 900,
        }
    };
    ResolvedElement {
        selector: selector.to_owned(),
        found,
        visible,
        interactable,
        confidence,
        bbox,
    }
}

/// Interactability gate for mutation dry-runs (no state mutation).
pub fn check_actionable(
    resolved: &ResolvedElement,
    action: &str,
    min_confidence: f64,
) -> Result<()> {
    if !resolved.found {
        return Err(err(&format!("{action}: element not found")));
    }
    if !resolved.visible {
        return Err(err(&format!("{action}: element not visible")));
    }
    if !resolved.interactable {
        return Err(err(&format!("{action}: element not interactable")));
    }
    if resolved.confidence < min_confidence {
        return Err(err(&format!(
            "low confidence {:.2} < {:.2}",
            resolved.confidence, min_confidence
        )));
    }
    Ok(())
}

/// Profile directory for one task's isolated computer session.
fn profile_dir(root: &std::path::Path, task_id: &str) -> std::path::PathBuf {
    root.join("computer-profiles").join(
        task_id
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect::<String>(),
    )
}

/// Ensure the isolated session exists (lifecycle: create on demand).
pub async fn ensure_profile(root: &std::path::Path, task_id: &str) -> Result<serde_json::Value> {
    if task_id.trim().is_empty() || task_id.len() > 128 {
        return Err(err("task_id must be 1..=128 chars"));
    }
    let dir = profile_dir(root, task_id);
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|error| err(&format!("computer profile: {error}")))?;
    let marker = serde_json::json!({
        "taskId": task_id,
        "persistent": false,
        "passwords": "never",
        "createdAt": talos_common::now_iso(),
    });
    tokio::fs::write(
        dir.join("session.json"),
        serde_json::to_string_pretty(&marker).unwrap_or_default(),
    )
    .await
    .map_err(|error| err(&format!("computer profile: {error}")))?;
    Ok(serde_json::json!({
        "profile": dir.to_string_lossy(),
        "persistent": false,
    }))
}

/// Deterministic mock computer: virtual screen, no OS calls.
#[derive(Debug, Clone, Default)]
pub struct MockComputer;

impl MockComputer {
    /// Fixed virtual-screen elements (plus one URL-seeded control).
    #[must_use]
    pub fn elements(url: Option<&str>) -> Vec<MockElement> {
        mock_elements(url)
    }

    /// Deterministic screen hash for a task + action + params fingerprint.
    #[must_use]
    pub fn screen_hash(task_id: &str, action: &str, fingerprint: &str) -> String {
        hash_hex(&format!(
            "screenshot:{task_id}:{action}:{fingerprint}:viewport=1440x900"
        ))
    }

    /// Planned move (dry-run): validate bounds, no state change.
    pub fn move_plan(x: i64, y: i64) -> serde_json::Value {
        serde_json::json!({
            "x": x,
            "y": y,
            "planned": true,
            "simulated": true,
        })
    }

    /// Planned click effects (dry-run: resolves, never clicks).
    pub fn click_plan(
        selector: Option<&str>,
        x: Option<i64>,
        y: Option<i64>,
    ) -> Result<serde_json::Value> {
        let (resolved, bbox) = Self::resolve_target(selector, x, y)?;
        Ok(serde_json::json!({
            "resolved": resolved,
            "boundingBox": bbox,
            "domHashless": true,
            "screenHash": hash_hex(&format!("click:{}:{:?}:{:?}", selector.unwrap_or(""), x, y)),
            "planned": true,
            "simulated": true,
        }))
    }

    /// Planned type effects (dry-run: validates, never types).
    pub fn type_plan(text: &str, selector: Option<&str>) -> Result<serde_json::Value> {
        if text.is_empty() || text.len() > 4096 {
            return Err(err("text must be 1..=4096 chars"));
        }
        let resolved = selector.map(|selector| {
            let report = resolve_selector(selector);
            serde_json::json!({
                "selector": report.selector,
                "found": report.found,
                "visible": report.visible,
                "interactable": report.interactable,
                "confidence": report.confidence,
            })
        });
        Ok(serde_json::json!({
            "chars": text.len(),
            "selector": selector,
            "resolved": resolved,
            "planned": true,
            "simulated": true,
        }))
    }

    /// Planned hotkey effects (dry-run: validates, never presses).
    pub fn hotkey_plan(keys: &str) -> Result<serde_json::Value> {
        if keys.trim().is_empty() || keys.len() > 64 {
            return Err(err("keys must be 1..=64 chars"));
        }
        Ok(serde_json::json!({
            "keys": keys,
            "planned": true,
            "simulated": true,
        }))
    }

    /// Live move (mock): profile + deterministic record.
    pub async fn move_live(
        root: &std::path::Path,
        task_id: &str,
        x: i64,
        y: i64,
    ) -> Result<serde_json::Value> {
        let profile = ensure_profile(root, task_id).await?;
        Ok(serde_json::json!({
            "x": x,
            "y": y,
            "screenHash": Self::screen_hash(task_id, "move_mouse", &format!("{x},{y}")),
            "profile": profile["profile"],
            "simulated": true,
        }))
    }

    /// Live click (mock): profile + resolution + deterministic click record.
    pub async fn click_live(
        root: &std::path::Path,
        task_id: &str,
        selector: Option<&str>,
        x: Option<i64>,
        y: Option<i64>,
        double: bool,
        min_confidence: f64,
    ) -> Result<serde_json::Value> {
        let profile = ensure_profile(root, task_id).await?;
        let (resolved, bbox) = Self::resolve_target(selector, x, y)?;
        Self::gate_confidence(
            &resolved,
            min_confidence,
            if double {
                "computer.double_click"
            } else {
                "computer.click"
            },
        )?;
        Ok(serde_json::json!({
            "resolved": resolved,
            "boundingBox": bbox,
            "screenHash": Self::screen_hash(task_id, if double { "double_click" } else { "click" }, &format!("{selector:?}:{x:?}:{y:?}")),
            "profile": profile["profile"],
            "simulated": true,
        }))
    }

    /// Live type (mock).
    pub async fn type_live(
        root: &std::path::Path,
        task_id: &str,
        selector: Option<&str>,
        text: &str,
        min_confidence: f64,
    ) -> Result<serde_json::Value> {
        if text.is_empty() || text.len() > 4096 {
            return Err(err("text must be 1..=4096 chars"));
        }
        let profile = ensure_profile(root, task_id).await?;
        if let Some(selector) = selector {
            let resolved = resolve_selector(selector);
            Self::gate_confidence_value(&resolved, min_confidence, "computer.type_text")?;
        }
        Ok(serde_json::json!({
            "chars": text.len(),
            "selector": selector,
            "screenHash": Self::screen_hash(task_id, "type_text", &format!("{}:{text}", selector.unwrap_or(""))),
            "profile": profile["profile"],
            "simulated": true,
        }))
    }

    /// Live hotkey (mock).
    pub async fn hotkey_live(
        root: &std::path::Path,
        task_id: &str,
        keys: &str,
    ) -> Result<serde_json::Value> {
        if keys.trim().is_empty() || keys.len() > 64 {
            return Err(err("keys must be 1..=64 chars"));
        }
        let profile = ensure_profile(root, task_id).await?;
        Ok(serde_json::json!({
            "keys": keys,
            "screenHash": Self::screen_hash(task_id, "hotkey", keys),
            "profile": profile["profile"],
            "simulated": true,
        }))
    }

    /// Live drag (mock).
    pub async fn drag_live(
        root: &std::path::Path,
        task_id: &str,
        x: i64,
        y: i64,
        x2: i64,
        y2: i64,
    ) -> Result<serde_json::Value> {
        let profile = ensure_profile(root, task_id).await?;
        Ok(serde_json::json!({
            "from": { "x": x, "y": y },
            "to": { "x": x2, "y": y2 },
            "screenHash": Self::screen_hash(task_id, "drag", &format!("{x},{y}->{x2},{y2}")),
            "profile": profile["profile"],
            "simulated": true,
        }))
    }

    /// Live scroll (mock).
    pub async fn scroll_live(
        root: &std::path::Path,
        task_id: &str,
        x: Option<i64>,
        y: Option<i64>,
        direction: &str,
        pixels: i64,
    ) -> Result<serde_json::Value> {
        let profile = ensure_profile(root, task_id).await?;
        Ok(serde_json::json!({
            "direction": direction,
            "pixels": pixels,
            "at": { "x": x, "y": y },
            "screenHash": Self::screen_hash(task_id, "scroll", &format!("{direction}:{pixels}")),
            "profile": profile["profile"],
            "simulated": true,
        }))
    }

    /// Capture the virtual screen (mock, read-only).
    #[must_use]
    pub fn capture_live(task_id: &str) -> serde_json::Value {
        serde_json::json!({
            "viewport": { "width": 1440, "height": 900 },
            "screenHash": Self::screen_hash(task_id, "capture_screen", "viewport"),
            "simulated": true,
        })
    }

    /// Locate an element (mock, read-only).
    pub fn locate_live(selector: &str, min_confidence: f64) -> Result<serde_json::Value> {
        let resolved = resolve_selector(selector);
        Self::gate_confidence_value(&resolved, min_confidence, "computer.locate_element")?;
        let bbox = Self::bbox_for(&resolved);
        Ok(serde_json::json!({
            "selector": selector,
            "resolved": {
                "selector": resolved.selector,
                "found": resolved.found,
                "visible": resolved.visible,
                "interactable": resolved.interactable,
                "confidence": resolved.confidence,
            },
            "boundingBox": bbox,
            "screenHash": hash_hex(&format!("locate:{selector}")),
            "simulated": true,
        }))
    }

    /// Resolve a click/double-click target to a report + bounding box.
    fn resolve_target(
        selector: Option<&str>,
        x: Option<i64>,
        y: Option<i64>,
    ) -> Result<(serde_json::Value, BoundingBox)> {
        if let Some(selector) = selector {
            let resolved = resolve_selector(selector);
            if !resolved.found {
                return Err(err("computer.click: element not found"));
            }
            let bbox = Self::bbox_for(&resolved);
            let report = serde_json::json!({
                "selector": resolved.selector,
                "found": resolved.found,
                "visible": resolved.visible,
                "interactable": resolved.interactable,
                "confidence": resolved.confidence,
            });
            return Ok((report, bbox));
        }
        match (x, y) {
            (Some(x), Some(y)) => Ok((
                serde_json::json!({ "x": x, "y": y, "point": true }),
                BoundingBox {
                    x,
                    y,
                    width: 1,
                    height: 1,
                },
            )),
            _ => Err(err("computer.click: selector or x/y required")),
        }
    }

    /// Bounding box for a resolved element.
    fn bbox_for(resolved: &ResolvedElement) -> BoundingBox {
        let hash = simple_hash(&resolved.selector);
        BoundingBox {
            x: (hash % 1100) as i64 + 20,
            y: (hash % 700) as i64 + 60,
            width: 80 + (hash % 200) as i64,
            height: 24 + (hash % 60) as i64,
        }
    }

    /// Confidence gate helper (struct form).
    fn gate_confidence(resolved: &serde_json::Value, min: f64, action: &str) -> Result<()> {
        let confidence = resolved["confidence"].as_f64().unwrap_or(0.0);
        if confidence < min {
            return Err(err(&format!(
                "low confidence {confidence:.2} < {min:.2} for {action}"
            )));
        }
        Ok(())
    }

    /// Confidence gate helper (typed form).
    fn gate_confidence_value(resolved: &ResolvedElement, min: f64, action: &str) -> Result<()> {
        if resolved.confidence < min {
            return Err(err(&format!(
                "low confidence {:.2} < {:.2} for {action}",
                resolved.confidence, min
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolution_reports_visibility_and_confidence() {
        let ok = resolve_selector("button#ok");
        assert!(ok.found && ok.visible && ok.interactable);
        assert!((0.5..=1.0).contains(&ok.confidence));
        let hidden = resolve_selector(".hidden-menu");
        assert!(hidden.found && !hidden.visible);
        assert!(check_actionable(&hidden, "computer.click", 0.7).is_err());
        let missing = resolve_selector(".missing-thing");
        assert!(!missing.found);
    }

    #[test]
    fn uncertain_selectors_yield_low_confidence() {
        let low = resolve_selector("button#uncertain-ok");
        assert!(low.confidence < 0.5);
        assert!(MockComputer::gate_confidence_value(&low, 0.7, "computer.click").is_err());
    }

    #[tokio::test]
    async fn profile_is_isolated_per_task() {
        let dir = tempfile::tempdir().expect("tempdir");
        ensure_profile(dir.path(), "task-a").await.expect("profile");
        assert!(dir
            .path()
            .join("computer-profiles/task-a/session.json")
            .exists());
        assert!(!dir.path().join("computer-profiles/task-b").exists());
    }

    #[test]
    fn screen_hash_is_deterministic() {
        let first = MockComputer::screen_hash("t", "click", "a");
        let second = MockComputer::screen_hash("t", "click", "a");
        assert_eq!(first, second);
        assert_ne!(first, MockComputer::screen_hash("t", "click", "b"));
        assert!(first.starts_with("sha256:"));
    }
}
