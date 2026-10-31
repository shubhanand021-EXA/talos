//! Safe browser-use tools over an isolated, deterministic mock browser.
//!
//! Safety model: every action runs inside a per-task profile directory under
//! the sandbox root (never a personal profile; no passwords, no persisted
//! cookies unless explicitly enabled). Dry-run resolves the selector and
//! validates the action without mutating anything. Live execution is a
//! deterministic simulation (`simulated: true`) — real Chromium arrives via
//! the `@talos/browser-use` worker; CI never needs a live browser.

use sha2::{Digest, Sha256};
use talos_common::{Result, TalosError};

use crate::adapters::SandboxConfig;

fn err(msg: &str) -> TalosError {
    TalosError::Storage(msg.to_owned())
}

fn hash_hex(input: &str) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(input.as_bytes())))
}

/// Profile directory for one task's isolated browser session.
fn profile_dir(config: &SandboxConfig, task_id: &str) -> std::path::PathBuf {
    config.root.join("browser-profiles").join(
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

/// Ensure the isolated profile exists (session lifecycle: create on demand).
///
/// Writes a `session.json` marker (`persistent: false` unless `persist`).
/// Never touches any profile outside the sandbox root.
pub async fn ensure_profile(
    config: &SandboxConfig,
    task_id: &str,
    persist: bool,
) -> Result<serde_json::Value> {
    if task_id.trim().is_empty() || task_id.len() > 128 {
        return Err(err("task_id must be 1..=128 chars"));
    }
    let dir = profile_dir(config, task_id);
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|error| err(&format!("browser profile: {error}")))?;
    let marker = serde_json::json!({
        "taskId": task_id,
        "persistent": persist,
        "passwords": "never",
        "createdAt": talos_common::now_iso(),
    });
    tokio::fs::write(
        dir.join("session.json"),
        serde_json::to_string_pretty(&marker).unwrap_or_default(),
    )
    .await
    .map_err(|error| err(&format!("browser profile: {error}")))?;
    Ok(serde_json::json!({
        "profile": dir.to_string_lossy(),
        "persistent": persist,
    }))
}

/// Deterministic fake DOM derived from the URL (seeded, no network).
fn mock_dom(url: &str, scope: Option<&str>) -> serde_json::Value {
    let seed = simple_hash(url);
    let scope_label = scope.unwrap_or("document");
    let mut nodes = Vec::new();
    for (index, label) in ["header", "main", "nav", "button#submit", "input#q"]
        .iter()
        .enumerate()
    {
        let visible = !label.contains("hidden");
        let id_seed = seed.wrapping_add(index as u64 * 2654435761);
        nodes.push(serde_json::json!({
            "role": label.split('#').next().unwrap_or("node"),
            "label": label,
            "selector": format!("{label}:nth({index})"),
            "visible": visible,
            "interactable": visible && (*label == "button#submit" || *label == "input#q"),
            "ref": format!("node-{id_seed:08x}"),
        }));
    }
    serde_json::json!({
        "url": url,
        "scope": scope_label,
        "title": format!("Mock page for {url}"),
        "nodes": nodes,
        "simulated": true,
    })
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

/// Canonical DOM hash (sha256 over the canonical JSON rendering).
pub fn dom_hash(dom: &serde_json::Value) -> String {
    hash_hex(&serde_json::to_string(dom).unwrap_or_default())
}

/// Resolve a selector against the mock DOM (deterministic report).
///
/// Selectors containing `hidden` resolve as present-but-not-visible;
/// selectors containing `missing` (or empty after trim) resolve as absent.
pub fn resolve_selector(url: &str, selector: &str, scope: Option<&str>) -> serde_json::Value {
    let dom = mock_dom(url, scope);
    let found = !selector.contains("missing");
    let visible = found && !selector.contains("hidden");
    let interactable = visible && !selector.contains("disabled");
    let match_count = if found {
        1 + (simple_hash(selector) % 3) as usize
    } else {
        0
    };
    serde_json::json!({
        "selector": selector,
        "found": found,
        "visible": visible,
        "interactable": interactable,
        "matchCount": match_count,
        "domHash": dom_hash(&dom),
    })
}

/// Interactability gate for mutation dry-runs (no page mutation).
fn check_actionable(resolved: &serde_json::Value, action: &str) -> Result<()> {
    if resolved["found"].as_bool() != Some(true) {
        return Err(err(&format!("{action}: selector not found")));
    }
    if resolved["visible"].as_bool() != Some(true) {
        return Err(err(&format!("{action}: element not visible")));
    }
    if resolved["interactable"].as_bool() != Some(true) {
        return Err(err(&format!("{action}: element not interactable")));
    }
    Ok(())
}

/// Planned navigate effects (dry-run: no session mutation).
#[must_use]
pub fn navigate_plan(url: &str, timeout_ms: u64) -> serde_json::Value {
    let dom = mock_dom(url, None);
    serde_json::json!({
        "url": url,
        "timeoutMs": timeout_ms,
        "domHash": dom_hash(&dom),
        "planned": true,
        "simulated": true,
    })
}

/// Live navigate (mock): ensure the isolated profile, return the DOM hash.
pub async fn navigate_live(
    config: &SandboxConfig,
    task_id: &str,
    url: &str,
    _timeout_ms: u64,
) -> Result<serde_json::Value> {
    let profile = ensure_profile(config, task_id, false).await?;
    let dom = mock_dom(url, None);
    Ok(serde_json::json!({
        "url": url,
        "domHash": dom_hash(&dom),
        "profile": profile["profile"],
        "simulated": true,
    }))
}

/// Planned click effects (dry-run: resolves, never clicks).
pub fn click_plan(
    url: Option<&str>,
    selector: &str,
    timeout_ms: u64,
    high_risk: bool,
    high_risk_kind: Option<&str>,
) -> Result<serde_json::Value> {
    let context = url.unwrap_or("(current page)");
    let resolved = resolve_selector(context, selector, None);
    check_actionable(&resolved, "browser.click")?;
    Ok(serde_json::json!({
        "selector": selector,
        "resolved": resolved,
        "domHash": resolved["domHash"],
        "timeoutMs": timeout_ms,
        "highRisk": high_risk,
        "highRiskKind": high_risk_kind,
        "planned": true,
        "simulated": true,
    }))
}

/// Live click (mock): profile + resolution + deterministic click record.
pub async fn click_live(
    config: &SandboxConfig,
    task_id: &str,
    url: Option<&str>,
    selector: &str,
    high_risk: bool,
    high_risk_kind: Option<&str>,
) -> Result<serde_json::Value> {
    let profile = ensure_profile(config, task_id, false).await?;
    let context = url.unwrap_or("(current page)");
    let resolved = resolve_selector(context, selector, None);
    check_actionable(&resolved, "browser.click")?;
    Ok(serde_json::json!({
        "selector": selector,
        "resolved": resolved,
        "domHash": resolved["domHash"],
        "profile": profile["profile"],
        "highRisk": high_risk,
        "highRiskKind": high_risk_kind,
        "simulated": true,
    }))
}

/// Planned type effects (dry-run: resolves, never types).
pub fn type_plan(
    url: Option<&str>,
    selector: &str,
    text: &str,
    submit: bool,
    timeout_ms: u64,
    high_risk: bool,
    high_risk_kind: Option<&str>,
) -> Result<serde_json::Value> {
    let context = url.unwrap_or("(current page)");
    let resolved = resolve_selector(context, selector, None);
    check_actionable(&resolved, "browser.type")?;
    Ok(serde_json::json!({
        "selector": selector,
        "chars": text.len(),
        "submit": submit,
        "resolved": resolved,
        "domHash": resolved["domHash"],
        "timeoutMs": timeout_ms,
        "highRisk": high_risk,
        "highRiskKind": high_risk_kind,
        "planned": true,
        "simulated": true,
    }))
}

/// Live type (mock): profile + resolution + deterministic record.
#[allow(clippy::too_many_arguments)]
pub async fn type_live(
    config: &SandboxConfig,
    task_id: &str,
    url: Option<&str>,
    selector: &str,
    text: &str,
    submit: bool,
    high_risk: bool,
    high_risk_kind: Option<&str>,
) -> Result<serde_json::Value> {
    let profile = ensure_profile(config, task_id, false).await?;
    let context = url.unwrap_or("(current page)");
    let resolved = resolve_selector(context, selector, None);
    check_actionable(&resolved, "browser.type")?;
    Ok(serde_json::json!({
        "selector": selector,
        "chars": text.len(),
        "submit": submit,
        "resolved": resolved,
        "domHash": resolved["domHash"],
        "profile": profile["profile"],
        "highRisk": high_risk,
        "highRiskKind": high_risk_kind,
        "simulated": true,
    }))
}

/// Planned scroll effects (dry-run).
#[must_use]
pub fn scroll_plan(url: Option<&str>, direction: &str, pixels: i64) -> serde_json::Value {
    let context = url.unwrap_or("(current page)");
    let dom = mock_dom(context, None);
    serde_json::json!({
        "direction": direction,
        "pixels": pixels,
        "domHash": dom_hash(&dom),
        "planned": true,
        "simulated": true,
    })
}

/// Live scroll (mock).
pub async fn scroll_live(
    config: &SandboxConfig,
    task_id: &str,
    url: Option<&str>,
    direction: &str,
    pixels: i64,
) -> Result<serde_json::Value> {
    let profile = ensure_profile(config, task_id, false).await?;
    let context = url.unwrap_or("(current page)");
    let dom = mock_dom(context, None);
    Ok(serde_json::json!({
        "direction": direction,
        "pixels": pixels,
        "domHash": dom_hash(&dom),
        "profile": profile["profile"],
        "simulated": true,
    }))
}

/// Planned select effects (dry-run: resolves, never selects).
pub fn select_plan(
    url: Option<&str>,
    selector: &str,
    value: Option<&str>,
    timeout_ms: u64,
) -> Result<serde_json::Value> {
    let context = url.unwrap_or("(current page)");
    let resolved = resolve_selector(context, selector, None);
    check_actionable(&resolved, "browser.select")?;
    Ok(serde_json::json!({
        "selector": selector,
        "value": value,
        "resolved": resolved,
        "domHash": resolved["domHash"],
        "timeoutMs": timeout_ms,
        "planned": true,
        "simulated": true,
    }))
}

/// Live select (mock).
pub async fn select_live(
    config: &SandboxConfig,
    task_id: &str,
    url: Option<&str>,
    selector: &str,
    value: Option<&str>,
) -> Result<serde_json::Value> {
    let profile = ensure_profile(config, task_id, false).await?;
    let context = url.unwrap_or("(current page)");
    let resolved = resolve_selector(context, selector, None);
    check_actionable(&resolved, "browser.select")?;
    Ok(serde_json::json!({
        "selector": selector,
        "value": value,
        "resolved": resolved,
        "domHash": resolved["domHash"],
        "profile": profile["profile"],
        "simulated": true,
    }))
}

/// Extract visible text (mock, bounded).
pub fn extract_live(url: &str, selector: Option<&str>, max_chars: usize) -> serde_json::Value {
    let dom = mock_dom(url, selector);
    let scope = selector.unwrap_or("document");
    let text = format!(
        "Mock extract of {url} [{scope}]: header, main content, navigation and form labels."
    );
    let capped: String = text.chars().take(max_chars).collect();
    serde_json::json!({
        "url": url,
        "text": capped,
        "textHash": hash_hex(&text),
        "domHash": dom_hash(&dom),
        "truncated": text.len() > max_chars,
        "simulated": true,
    })
}

/// Screenshot hash (mock: deterministic, no pixels stored).
pub fn screenshot_live(url: &str, full_page: bool) -> serde_json::Value {
    let dom = mock_dom(url, None);
    serde_json::json!({
        "url": url,
        "screenshotHash": hash_hex(&format!("screenshot:{url}:full={full_page}:viewport=1440x900")),
        "domHash": dom_hash(&dom),
        "fullPage": full_page,
        "simulated": true,
    })
}

/// DOM snapshot (mock).
pub fn snapshot_live(url: &str, selector: Option<&str>) -> serde_json::Value {
    let dom = mock_dom(url, selector);
    serde_json::json!({
        "url": url,
        "dom": dom,
        "domHash": dom_hash(&dom),
        "simulated": true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dom_is_deterministic() {
        let first = mock_dom("https://example.com", None);
        let second = mock_dom("https://example.com", None);
        assert_eq!(dom_hash(&first), dom_hash(&second));
        assert_ne!(
            dom_hash(&first),
            dom_hash(&mock_dom("https://other.example", None))
        );
    }

    #[test]
    fn selector_resolution_reports_visibility() {
        let ok = resolve_selector("https://example.com", "button#ok", None);
        assert_eq!(ok["found"], true);
        assert_eq!(ok["visible"], true);
        assert_eq!(ok["interactable"], true);
        let hidden = resolve_selector("https://example.com", ".hidden-menu", None);
        assert_eq!(hidden["found"], true);
        assert_eq!(hidden["visible"], false);
        assert!(check_actionable(&hidden, "browser.click").is_err());
        let missing = resolve_selector("https://example.com", ".missing-thing", None);
        assert_eq!(missing["found"], false);
    }

    #[tokio::test]
    async fn profile_is_isolated_per_task() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = SandboxConfig {
            root: dir.path().to_path_buf(),
            ..SandboxConfig::default()
        };
        ensure_profile(&config, "task-a", false)
            .await
            .expect("profile");
        assert!(dir
            .path()
            .join("browser-profiles/task-a/session.json")
            .exists());
        assert!(!dir.path().join("browser-profiles/task-b").exists());
    }

    #[test]
    fn screenshot_and_extract_are_bounded() {
        let shot = screenshot_live("https://example.com", false);
        assert!(shot["screenshotHash"]
            .as_str()
            .unwrap_or("")
            .starts_with("sha256:"));
        let text = extract_live("https://example.com", None, 10);
        assert!(text["text"].as_str().unwrap_or("").len() <= 10);
        assert_eq!(text["truncated"], true);
    }
}
