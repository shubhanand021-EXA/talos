//! Persistent browser profiles managed by the Gateway.
//!
//! Layout: `<root>/<sanitized-name>/` holding a Chromium `user_data_dir`
//! (`session.json` marker with `persistent: true`, `passwords: "never"`).
//! The default root is `~/.talos/browser_profiles`, overridable with
//! `TALOS_BROWSER_PROFILE_ROOT` (tests and portable installs).
//!
//! Profiles are per-*name*, not per-task: the Browser Agent reuses the
//! `default` profile across tasks so logins and cookies persist. Clearing a
//! profile deletes its directory (cookies + storage gone, next launch is a
//! clean slate). Nothing here ever stores passwords.

use std::path::PathBuf;

use talos_common::{Result, TalosError};

/// Default profile name reused across tasks (`~/.talos/browser_profiles/default`).
pub const DEFAULT_PROFILE_NAME: &str = "default";
/// Environment override for the profile root (tests, portable installs).
pub const PROFILE_ROOT_ENV: &str = "TALOS_BROWSER_PROFILE_ROOT";
/// Marker file written into every managed profile directory.
const MARKER_FILE: &str = "session.json";

fn err(msg: impl Into<String>) -> TalosError {
    TalosError::Storage(msg.into())
}

/// Gateway-managed profile: a named, persistent Chromium `user_data_dir`.
///
/// Mirrors `@talos/contracts` `BrowserProfileSchema`: `{ name, userDataDir,
/// persistent, createdAt }` (camelCase on the wire).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserProfile {
    /// Profile name (`1..=64` chars, `[a-zA-Z0-9_-]`).
    pub name: String,
    /// Absolute `user_data_dir` for the Playwright persistent context.
    pub user_data_dir: PathBuf,
    /// Always true for Gateway-managed profiles (never task-ephemeral).
    pub persistent: bool,
    /// RFC 3339 creation timestamp.
    pub created_at: String,
}

/// Resolve the Gateway profile root: `TALOS_BROWSER_PROFILE_ROOT` when set,
/// otherwise `~/.talos/browser_profiles` (`%USERPROFILE%` on Windows).
#[must_use]
pub fn gateway_profile_root() -> PathBuf {
    if let Ok(root) = std::env::var(PROFILE_ROOT_ENV) {
        if !root.trim().is_empty() {
            return PathBuf::from(root);
        }
    }
    talos_home().join("browser_profiles")
}

fn talos_home() -> PathBuf {
    if let Ok(home) = std::env::var("TALOS_DATA_DIR") {
        if !home.trim().is_empty() {
            return PathBuf::from(home);
        }
    }
    #[cfg(windows)]
    if let Ok(profile) = std::env::var("USERPROFILE") {
        if !profile.trim().is_empty() {
            return PathBuf::from(profile).join(".talos");
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        if !home.trim().is_empty() {
            return PathBuf::from(home).join(".talos");
        }
    }
    std::env::temp_dir().join("talos")
}

/// Validate + sanitize a profile name (`1..=64` chars, `[a-zA-Z0-9_-]`).
pub fn sanitize_profile_name(name: &str) -> Result<String> {
    if name.trim().is_empty() || name.len() > 64 {
        return Err(err("profile name must be 1..=64 chars"));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(err("profile name allowed chars: a-z A-Z 0-9 - _"));
    }
    Ok(name.to_owned())
}

/// Directory for one named profile under `root`.
#[must_use]
pub fn profile_dir(root: &std::path::Path, name: &str) -> PathBuf {
    root.join(name)
}

/// Ensure a persistent profile exists (create the `user_data_dir` + marker).
///
/// Idempotent: re-ensuring a live profile keeps cookies/storage and refreshes
/// nothing. Never touches directories outside `root`.
pub async fn ensure_profile(root: &std::path::Path, name: &str) -> Result<BrowserProfile> {
    let name = sanitize_profile_name(name)?;
    let dir = profile_dir(root, &name);
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|error| err(format!("browser profile: {error}")))?;
    let marker_path = dir.join(MARKER_FILE);
    let created_at = if let Ok(raw) = tokio::fs::read_to_string(&marker_path).await {
        serde_json::from_str::<serde_json::Value>(&raw)
            .ok()
            .and_then(|value| {
                value
                    .get("createdAt")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned)
            })
            .unwrap_or_else(talos_common::now_iso)
    } else {
        talos_common::now_iso()
    };
    let marker = serde_json::json!({
        "name": name,
        "persistent": true,
        "passwords": "never",
        "createdAt": created_at,
    });
    tokio::fs::write(
        &marker_path,
        serde_json::to_string_pretty(&marker).unwrap_or_default(),
    )
    .await
    .map_err(|error| err(format!("browser profile: {error}")))?;
    Ok(BrowserProfile {
        name,
        user_data_dir: dir,
        persistent: true,
        created_at,
    })
}

/// List Gateway-managed profiles under `root` (missing root ⇒ empty list).
pub async fn list_profiles(root: &std::path::Path) -> Result<Vec<BrowserProfile>> {
    let mut out = Vec::new();
    let mut entries = match tokio::fs::read_dir(root).await {
        Ok(entries) => entries,
        Err(_) => return Ok(out),
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let dir = entry.path();
        if !entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let Some(name) = dir.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if sanitize_profile_name(name).is_err() {
            continue;
        }
        let created_at = tokio::fs::read_to_string(dir.join(MARKER_FILE))
            .await
            .ok()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
            .and_then(|value| {
                value
                    .get("createdAt")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned)
            })
            .unwrap_or_else(talos_common::now_iso);
        out.push(BrowserProfile {
            name: name.to_owned(),
            user_data_dir: dir,
            persistent: true,
            created_at,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Clear a profile: delete its `user_data_dir` (cookies + storage gone).
/// Returns `true` when a directory was removed, `false` when absent.
/// Never touches paths outside `root`.
pub async fn clear_profile(root: &std::path::Path, name: &str) -> Result<bool> {
    let name = sanitize_profile_name(name)?;
    let dir = profile_dir(root, &name);
    match tokio::fs::remove_dir_all(&dir).await {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(err(format!("browser profile clear: {error}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "talos-browser-profile-test-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("tmpdir");
        dir
    }

    #[tokio::test]
    async fn profile_persistence_round_trip() {
        let root = tmp_root("persist");
        let first = ensure_profile(&root, "default").await.expect("ensure");
        assert!(first.persistent);
        assert!(first.user_data_dir.join(MARKER_FILE).exists());
        // Simulate persisted state: drop a cookie jar, re-ensure, confirm kept.
        tokio::fs::write(first.user_data_dir.join("Cookies"), b"jar-bytes")
            .await
            .expect("jar");
        let second = ensure_profile(&root, "default").await.expect("re-ensure");
        assert_eq!(first.user_data_dir, second.user_data_dir);
        assert_eq!(first.created_at, second.created_at);
        assert!(
            second.user_data_dir.join("Cookies").exists(),
            "cookies survive re-ensure"
        );
        let listed = list_profiles(&root).await.expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "default");
    }

    #[tokio::test]
    async fn clear_profile_removes_state() {
        let root = tmp_root("clear");
        ensure_profile(&root, "default").await.expect("ensure");
        assert!(clear_profile(&root, "default").await.expect("clear"));
        assert!(!clear_profile(&root, "default").await.expect("clear again"));
        assert!(list_profiles(&root).await.expect("list").is_empty());
        // A fresh ensure after clear is a clean slate.
        let fresh = ensure_profile(&root, "default").await.expect("re-ensure");
        assert!(!fresh.user_data_dir.join("Cookies").exists());
    }

    #[tokio::test]
    async fn rejects_bad_names_and_escapes() {
        let root = tmp_root("names");
        assert!(ensure_profile(&root, "").await.is_err());
        assert!(ensure_profile(&root, "../evil").await.is_err());
        assert!(ensure_profile(&root, "a/b").await.is_err());
        assert!(ensure_profile(&root, "has space").await.is_err());
        assert!(ensure_profile(&root, &"x".repeat(65)).await.is_err());
        assert!(ensure_profile(&root, "ok-name_1").await.is_ok());
    }

    #[test]
    fn root_env_override_wins() {
        let before = std::env::var(PROFILE_ROOT_ENV).ok();
        std::env::set_var(PROFILE_ROOT_ENV, "/tmp/talos-profiles-test");
        assert_eq!(
            gateway_profile_root(),
            PathBuf::from("/tmp/talos-profiles-test")
        );
        match before {
            Some(value) => std::env::set_var(PROFILE_ROOT_ENV, value),
            None => std::env::remove_var(PROFILE_ROOT_ENV),
        }
    }

    #[test]
    fn contract_shape_is_camel_case() {
        let profile = BrowserProfile {
            name: "default".to_owned(),
            user_data_dir: PathBuf::from("/tmp/x"),
            persistent: true,
            created_at: "2026-11-08T00:00:00Z".to_owned(),
        };
        let raw = serde_json::to_value(&profile).expect("json");
        assert_eq!(raw["userDataDir"], "/tmp/x");
        assert_eq!(raw["createdAt"], "2026-11-08T00:00:00Z");
        assert!(raw.get("user_data_dir").is_none());
    }
}
