//! Persistent browser profiles + HITL CAPTCHA/auth bridge (desktop shell).
//!
//! IPC surface (`IpcCommandName.listBrowserProfiles`, `clearBrowserProfile`,
//! `resumeBrowserHitl`):
//! - `list_browser_profiles` — Gateway-managed profiles under
//!   `~/.talos/browser_profiles` (metadata only, never cookies).
//! - `clear_browser_profile` — delete one profile's `user_data_dir`.
//! - `resume_browser_hitl` — release a paused `browser.wait_for_human` gate
//!   (`resumed: true` continues the agent, `false` dismisses it).
//! - `pending_browser_hitl` — snapshot of paused gates (modal queue).
//!
//! Secret injection: `browser.session launch` accepts `cookieSecrets` /
//! `authHeaderSecrets` (keychain key *names*). This module resolves the
//! values from the OS keychain and stages them as a `0600`
//! `<profile_dir>/injections.json` handoff the Playwright worker seeds into
//! the fresh context. Values never touch IPC responses, receipts, events,
//! or logs — key names at most.

use std::collections::HashMap;

use talos_common::TalosError;

use crate::commands::IpcError;
use crate::events::{emit_talos_event, BROWSER_HITL_REQUIRED};
use crate::state::AppState;

/// Install the pause-request fan-out: every HITL gate registration emits a
/// `browserHitlRequired` Tauri event so the Desktop UI renders the
/// intervention modal (payload is linkage + reason only, never page content).
pub fn start_browser_hitl_fanout(app: &tauri::AppHandle) {
    let handle = app.clone();
    browser_use::global_registry().set_listener(std::sync::Arc::new(move |event| {
        let payload = serde_json::json!({
            "interventionId": event.intervention_id,
            "taskId": event.task_id,
            "kind": event.kind.as_str(),
            "url": event.url,
            "reason": event.reason,
            "checkedAt": talos_common::now_iso(),
        });
        if let Err(err) = emit_talos_event(&handle, BROWSER_HITL_REQUIRED, payload) {
            eprintln!("[talos] event emission failed ({BROWSER_HITL_REQUIRED}): {err}");
        }
    }));
}

/// `list_browser_profiles` — Gateway-managed persistent profiles.
#[tauri::command]
pub async fn list_browser_profiles() -> Result<Vec<browser_use::BrowserProfile>, IpcError> {
    browser_use::list_profiles(&browser_use::gateway_profile_root())
        .await
        .map_err(IpcError::from)
}

/// `clear_browser_profile` — delete one profile's `user_data_dir`.
#[tauri::command]
pub async fn clear_browser_profile(profile: String) -> Result<bool, IpcError> {
    let name = browser_use::sanitize_profile_name(&profile).map_err(IpcError::from)?;
    browser_use::clear_profile(&browser_use::gateway_profile_root(), &name)
        .await
        .map_err(IpcError::from)
}

/// `resume_browser_hitl` — release a paused browser gate.
#[tauri::command]
pub async fn resume_browser_hitl(
    intervention_id: String,
    resumed: Option<bool>,
) -> Result<bool, IpcError> {
    if intervention_id.trim().is_empty() || intervention_id.len() > 128 {
        return Err(IpcError::from(TalosError::InvalidId {
            what: "intervention_id",
            reason: "must be 1..=128 chars".to_owned(),
        }));
    }
    let outcome = if resumed.unwrap_or(true) {
        browser_use::HitlOutcome::Resumed
    } else {
        browser_use::HitlOutcome::Dismissed
    };
    if browser_use::global_registry().resolve(&intervention_id, outcome) {
        Ok(true)
    } else {
        Err(IpcError::from(TalosError::Storage(
            "unknown_intervention: no pending browser intervention with that id".to_owned(),
        )))
    }
}

/// `pending_browser_hitl` — snapshot of paused gates (modal queue).
#[tauri::command]
pub async fn pending_browser_hitl() -> Result<Vec<browser_use::HitlBrowserEvent>, IpcError> {
    Ok(browser_use::global_registry().pending())
}

/// Parsed keychain key name: `browser.{cookie|header}.{profile}.{host}.{name}`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SecretRef {
    /// `cookie` or `header`.
    kind: String,
    /// Profile the value belongs to.
    profile: String,
    /// Host scope (may contain dots).
    host: String,
    /// Cookie/header name.
    name: String,
}

/// Split a `browser.cookie|header.{profile}.{host}.{name}` key name.
fn parse_secret_key(key: &str) -> Result<SecretRef, IpcError> {
    let parts: Vec<&str> = key.split('.').collect();
    if parts.len() < 5 || parts[0] != "browser" {
        return Err(IpcError::from(TalosError::InvalidId {
            what: "secret key_name",
            reason: "expected browser.{cookie|header}.{profile}.{host}.{name}".to_owned(),
        }));
    }
    let kind = parts[1].to_owned();
    if kind != "cookie" && kind != "header" {
        return Err(IpcError::from(TalosError::InvalidId {
            what: "secret key_name",
            reason: "expected browser.{cookie|header}.{profile}.{host}.{name}".to_owned(),
        }));
    }
    let profile = browser_use::sanitize_profile_name(parts[2]).map_err(IpcError::from)?;
    let name = parts[parts.len() - 1].to_owned();
    if name.trim().is_empty() {
        return Err(IpcError::from(TalosError::InvalidId {
            what: "secret key_name",
            reason: "cookie/header name must not be empty".to_owned(),
        }));
    }
    let host = parts[3..parts.len() - 1].join(".");
    if host.trim().is_empty() {
        return Err(IpcError::from(TalosError::InvalidId {
            what: "secret key_name",
            reason: "host must not be empty".to_owned(),
        }));
    }
    Ok(SecretRef {
        kind,
        profile,
        host,
        name,
    })
}

/// Stage keychain secrets for a `browser.session launch`.
///
/// Called from `invoke_tool_with` before the policy-gated invocation:
/// - ensures the target Gateway profile exists,
/// - resolves every `cookieSecrets` / `authHeaderSecrets` key name from the
///   OS keychain (missing entries fail closed),
/// - writes a `0600` `injections.json` handoff per involved profile dir.
///
/// Values travel keychain → RAM → handoff file only. This function returns
/// `()` — nothing carrying values ever reaches the caller.
pub async fn prepare_browser_launch(
    state: &tauri::State<'_, AppState>,
    params: &serde_json::Value,
) -> Result<(), IpcError> {
    let operation = params
        .get("operation")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if operation != "launch" {
        return Ok(());
    }
    let profile = params
        .get("profile")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(browser_use::DEFAULT_PROFILE_NAME);
    let root = browser_use::gateway_profile_root();
    // Always ensure the launch profile so the `user_data_dir` exists even
    // for mock runs (the live worker reuses it verbatim).
    let launch_dir = browser_use::ensure_profile(&root, profile)
        .await
        .map_err(IpcError::from)?
        .user_data_dir;
    void_profile_dir(&launch_dir);
    let cookie_keys = secret_names(params, "cookieSecrets")?;
    let header_keys = secret_names(params, "authHeaderSecrets")?;
    if cookie_keys.is_empty() && header_keys.is_empty() {
        return Ok(());
    }
    // Group by (profile, host): one handoff file per involved profile dir.
    let mut cookies_by_scope: HashMap<(String, String), Vec<browser_use::CookieInjection>> =
        HashMap::new();
    let mut headers_by_scope: HashMap<(String, String), Vec<browser_use::HeaderInjection>> =
        HashMap::new();
    for key in cookie_keys.iter().chain(header_keys.iter()) {
        let parsed = parse_secret_key(key)?;
        let secret = state
            .secrets
            .get_secret(talos_secrets::TALOS_SERVICE, key)
            .map_err(IpcError::from)?
            .ok_or_else(|| {
                IpcError::from(TalosError::Storage(format!(
                    "cookie secret not found: {key}"
                )))
            })?;
        // Secret values stay in these locals; only names leave this function.
        if parsed.kind == "cookie" {
            cookies_by_scope
                .entry((parsed.profile.clone(), parsed.host.clone()))
                .or_default()
                .push(browser_use::CookieInjection {
                    name: parsed.name.clone(),
                    value: secret,
                    domain: parsed.host.clone(),
                    path: "/".to_owned(),
                });
        } else {
            headers_by_scope
                .entry((parsed.profile.clone(), parsed.host.clone()))
                .or_default()
                .push(browser_use::HeaderInjection {
                    name: parsed.name.clone(),
                    value: secret,
                });
        }
    }
    let mut scopes: Vec<(String, String)> = cookies_by_scope
        .keys()
        .chain(headers_by_scope.keys())
        .cloned()
        .collect();
    scopes.sort();
    scopes.dedup();
    for (scope_profile, scope_host) in scopes {
        let dir = browser_use::ensure_profile(&root, &scope_profile)
            .await
            .map_err(IpcError::from)?
            .user_data_dir;
        let plan = browser_use::InjectionPlan {
            cookies: cookies_by_scope
                .remove(&(scope_profile.clone(), scope_host.clone()))
                .unwrap_or_default(),
            headers: headers_by_scope
                .remove(&(scope_profile.clone(), scope_host.clone()))
                .unwrap_or_default(),
            profile: scope_profile,
            host: scope_host,
        };
        browser_use::write_injection_file(&dir, &plan)
            .await
            .map_err(IpcError::from)?;
    }
    Ok(())
}

/// Read a bounded list of keychain key *names* from tool params.
fn secret_names(params: &serde_json::Value, key: &'static str) -> Result<Vec<String>, IpcError> {
    let Some(list) = params.get(key) else {
        return Ok(Vec::new());
    };
    let entries = list.as_array().ok_or_else(|| {
        IpcError::from(TalosError::InvalidId {
            what: key,
            reason: "must be an array of key names".to_owned(),
        })
    })?;
    if entries.len() > 16 {
        return Err(IpcError::from(TalosError::InvalidId {
            what: key,
            reason: "must have at most 16 entries".to_owned(),
        }));
    }
    entries
        .iter()
        .map(|entry| {
            entry.as_str().map(str::to_owned).ok_or_else(|| {
                IpcError::from(TalosError::InvalidId {
                    what: key,
                    reason: "entries must be strings".to_owned(),
                })
            })
        })
        .collect()
}

/// Marker: the launch profile dir is resolved (and created) for its side
/// effect; the path itself is reported through the worker receipt, not here.
fn void_profile_dir(_dir: &std::path::Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_key_names_parse() {
        let parsed =
            parse_secret_key("browser.cookie.default.example.com.session").expect("cookie");
        assert_eq!(
            parsed,
            SecretRef {
                kind: "cookie".to_owned(),
                profile: "default".to_owned(),
                host: "example.com".to_owned(),
                name: "session".to_owned(),
            }
        );
        let parsed = parse_secret_key("browser.header.work.sub.example.co.uk.Authorization")
            .expect("header");
        assert_eq!(parsed.host, "sub.example.co.uk");
        assert_eq!(parsed.name, "Authorization");
        assert!(parse_secret_key("provider.openai.api_key").is_err());
        assert!(parse_secret_key("browser.cookie.default..name").is_err());
        assert!(parse_secret_key("browser.cookie...").is_err());
        assert!(parse_secret_key("browser.token.default.example.com.x").is_err());
    }

    #[test]
    fn secret_name_lists_are_bounded() {
        assert!(secret_names(&serde_json::json!({}), "cookieSecrets")
            .expect("absent")
            .is_empty());
        assert!(secret_names(
            &serde_json::json!({ "cookieSecrets": ["a.b"] }),
            "cookieSecrets"
        )
        .is_ok());
        let too_many: Vec<String> = (0..17).map(|i| format!("k{i}")).collect();
        assert!(secret_names(
            &serde_json::json!({ "cookieSecrets": too_many }),
            "cookieSecrets"
        )
        .is_err());
    }
}
