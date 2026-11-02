//! Signed desktop updater IPC: check for updates, download + install.
//!
//! Updates are served over staged-rollout endpoints
//! (`https://updates.talos.dev/{{target}}/{{arch}}/{{current_version}}`,
//! placeholder until the first release channel exists) and verified with the
//! ed25519 public key in `tauri.conf.json` (`plugins.updater.pubkey`).
//! Release bundles are signed with `scripts/sign-release.mjs`; the private
//! key must never be committed to the repo.
//!
//! `check_for_updates` reports availability without downloading anything.
//! `install_update` downloads, verifies, installs, and restarts the app.

use talos_common::TalosError;

use crate::commands::IpcError;

/// Update status surfaced to the UI (camelCase JSON).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfoView {
    /// Whether a newer release is available.
    pub available: bool,
    /// Running version.
    pub current_version: String,
    /// Latest release version, when an update is available.
    pub latest_version: Option<String>,
    /// Release notes, when an update is available.
    pub notes: Option<String>,
    /// Release publication date (RFC 3339), when known.
    pub pub_date: Option<String>,
    /// Artifact URL the update would download, when known.
    pub download_url: Option<String>,
}

/// Expand an update-endpoint template (`{{target}}`, `{{arch}}`,
/// `{{current_version}}`) for display and diagnostics.
#[must_use]
pub fn expand_endpoint_template(
    template: &str,
    target: &str,
    arch: &str,
    current_version: &str,
) -> String {
    template
        .replace("{{target}}", target)
        .replace("{{arch}}", arch)
        .replace("{{current_version}}", current_version)
}

/// Placeholder update endpoint until the first release channel exists.
pub const UPDATE_ENDPOINT_TEMPLATE: &str =
    "https://updates.talos.dev/{{target}}/{{arch}}/{{current_version}}";

/// `check_for_updates` — query the update endpoint; never downloads.
#[tauri::command]
pub async fn check_for_updates(app: tauri::AppHandle) -> Result<UpdateInfoView, IpcError> {
    use tauri_plugin_updater::UpdaterExt as _;
    let current_version = app.package_info().version.to_string();
    let update = app
        .updater()
        .map_err(|err| IpcError::from(TalosError::Internal(format!("updater unavailable: {err}"))))?
        .check()
        .await
        .map_err(|err| {
            // Name the concrete endpoint so a failed check is diagnosable
            // without guessing which staged rollout was queried.
            let endpoint = expand_endpoint_template(
                UPDATE_ENDPOINT_TEMPLATE,
                std::env::consts::OS,
                std::env::consts::ARCH,
                &current_version,
            );
            IpcError::from(TalosError::Internal(format!(
                "update check failed ({endpoint}): {err}"
            )))
        })?;
    Ok(match update {
        Some(update) => UpdateInfoView {
            available: true,
            current_version,
            latest_version: Some(update.version.clone()),
            notes: update.body.clone(),
            pub_date: update.date.map(|d| {
                d.format(&time::format_description::well_known::Rfc3339)
                    .unwrap_or_else(|_| d.to_string())
            }),
            download_url: Some(update.download_url.to_string()),
        },
        None => UpdateInfoView {
            available: false,
            current_version,
            latest_version: None,
            notes: None,
            pub_date: None,
            download_url: None,
        },
    })
}

/// `install_update` — download, verify (ed25519), install, and restart.
///
/// The plugin verifies the bundle signature against the configured pubkey
/// before touching the installation; a bad signature aborts the install.
#[tauri::command]
pub async fn install_update(app: tauri::AppHandle) -> Result<(), IpcError> {
    use tauri_plugin_updater::UpdaterExt as _;
    let update = app
        .updater()
        .map_err(|err| IpcError::from(TalosError::Internal(format!("updater unavailable: {err}"))))?
        .check()
        .await
        .map_err(|err| IpcError::from(TalosError::Internal(format!("update check failed: {err}"))))?
        .ok_or_else(|| IpcError::from(TalosError::Storage("no update available".to_owned())))?;
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|err| {
            IpcError::from(TalosError::Internal(format!(
                "update install failed: {err}"
            )))
        })?;
    app.restart();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_template_expands() {
        assert_eq!(
            expand_endpoint_template(
                UPDATE_ENDPOINT_TEMPLATE,
                "windows-x86_64",
                "x86_64",
                "0.3.0"
            ),
            "https://updates.talos.dev/windows-x86_64/x86_64/0.3.0"
        );
        // Unknown placeholders pass through untouched (`.test` TLD: RFC 2606
        // documentation-only, never a runtime target).
        assert_eq!(
            expand_endpoint_template("https://updater.test/{{nope}}", "t", "a", "v"),
            "https://updater.test/{{nope}}"
        );
    }

    #[test]
    fn update_info_view_serializes_camel_case() {
        let view = UpdateInfoView {
            available: true,
            current_version: "0.3.0".to_owned(),
            latest_version: Some("0.4.0".to_owned()),
            notes: Some("hardening".to_owned()),
            pub_date: Some("2026-11-02T00:00:00Z".to_owned()),
            download_url: Some("https://updates.talos.dev/x".to_owned()),
        };
        let json = serde_json::to_value(&view).expect("serializes");
        assert_eq!(json["available"], true);
        assert_eq!(json["latestVersion"], "0.4.0");
        assert_eq!(json["downloadUrl"], "https://updates.talos.dev/x");
        let idle = UpdateInfoView {
            available: false,
            current_version: "0.4.0".to_owned(),
            latest_version: None,
            notes: None,
            pub_date: None,
            download_url: None,
        };
        let json = serde_json::to_value(&idle).expect("serializes");
        assert_eq!(json["available"], false);
    }
}
