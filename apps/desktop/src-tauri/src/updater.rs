//! Signed desktop updater IPC: staged channels, download staging, install.
//!
//! Updates are served per release channel and verified twice: the release
//! manifest carries a minisign signature over its canonical JSON (see
//! [`talos_updates::verify_manifest_signature`], same `.sig` format produced
//! offline by `scripts/sign-release.mjs`), and every bundle the Tauri plugin
//! installs is ed25519-verified against the public key pinned in
//! `tauri.conf.json` (`plugins.updater.pubkey`) before it touches the
//! installation.
//!
//! Flow: [`check_for_updates`] reports availability without downloading.
//! [`download_update`] stages bundle bytes in the OS temp dir (recorded in
//! `AppState::pending_update`). [`install_update`] prefers staged bytes for
//! the same version (installing them with [`Update::install`], which still
//! verifies the bundle signature) and falls back to `download_and_install`
//! otherwise, then restarts.

use talos_common::TalosError;
use talos_updates::{install_id, read_channel, UpdateChannel};

use crate::commands::IpcError;
use crate::state::AppState;

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

/// Rich update status surfaced to the UI (camelCase JSON).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatusView {
    /// Base availability info.
    #[serde(flatten)]
    pub info: UpdateInfoView,
    /// Release channel this status was evaluated for.
    pub channel: String,
    /// Whether this install is inside the staged-rollout cohort.
    pub rollout_applicable: bool,
    /// Whether bundle bytes are already staged locally.
    pub staged: bool,
    /// Staged version, when bytes are staged.
    pub staged_version: Option<String>,
    /// When the staged bytes finished downloading (RFC 3339).
    pub staged_downloaded_at: Option<String>,
}

/// A staged (downloaded, not yet installed) update.
#[derive(Debug, Clone)]
pub struct StagedUpdate {
    /// Release version the staged bytes belong to.
    pub version: String,
    /// Temp file holding the verified bundle bytes.
    pub path: std::path::PathBuf,
    /// When the download completed (RFC 3339).
    pub downloaded_at: String,
}

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
            let endpoint = talos_updates::expand_endpoint_template(
                talos_updates::UPDATE_ENDPOINT_TEMPLATE,
                talos_updates::DEFAULT_CHANNEL,
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

/// `get_update_status` — availability plus channel, rollout cohort, and
/// staged-download state (still never downloads).
#[tauri::command]
pub async fn get_update_status(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<UpdateStatusView, IpcError> {
    let info = check_for_updates(app).await?;
    let channel = read_channel(&state.db).await;
    let id = install_id(&state.db)
        .await
        .map_err(|err| IpcError::from(TalosError::Storage(err.to_string())))?;
    // Without a cached manifest the client assumes full rollout: the plugin
    // endpoint only serves builds this install is eligible for.
    let rollout_applicable = talos_updates::rollout_applies(None, &id);
    let staged = state.pending_update.lock().await.clone();
    let staged_current = staged.as_ref().is_some_and(|s| {
        info.latest_version
            .as_deref()
            .is_some_and(|latest| latest == s.version)
    });
    Ok(UpdateStatusView {
        info,
        channel,
        rollout_applicable,
        staged: staged_current,
        staged_version: staged_current
            .then(|| staged.clone().map(|s| s.version))
            .flatten(),
        staged_downloaded_at: staged_current
            .then(|| staged.map(|s| s.downloaded_at))
            .flatten(),
    })
}

/// `download_update` — download the pending bundle and stage it in the OS
/// temp dir (signature-verified by the plugin during download, exactly like
/// `install_update`; staging only defers the restart, never the check).
#[tauri::command]
pub async fn download_update(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<UpdateStatusView, IpcError> {
    use tauri_plugin_updater::UpdaterExt as _;
    let update = app
        .updater()
        .map_err(|err| IpcError::from(TalosError::Internal(format!("updater unavailable: {err}"))))?
        .check()
        .await
        .map_err(|err| IpcError::from(TalosError::Internal(format!("update check failed: {err}"))))?
        .ok_or_else(|| IpcError::from(TalosError::Storage("no update available".to_owned())))?;
    let version = update.version.clone();
    let bytes = update.download(|_, _| {}, || {}).await.map_err(|err| {
        IpcError::from(TalosError::Internal(format!(
            "update download failed: {err}"
        )))
    })?;
    // Stage to a temp file (install reads it back; a restart never loses a
    // staged update mid-flight because the path is re-resolved on demand).
    let path = std::env::temp_dir().join(format!("talos-update-{version}.bin"));
    std::fs::write(&path, &bytes)
        .map_err(|err| IpcError::from(TalosError::Storage(format!("stage update: {err}"))))?;
    *state.pending_update.lock().await = Some(StagedUpdate {
        version: version.clone(),
        path,
        downloaded_at: talos_common::now_iso(),
    });
    Box::pin(get_update_status(app, state)).await
}

/// `install_update` — install staged bytes when fresh, else download +
/// verify + install, then restart.
///
/// The bundle signature is verified against the pinned pubkey before the
/// installation touches anything; a bad signature aborts the install.
#[tauri::command]
pub async fn install_update(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), IpcError> {
    use tauri_plugin_updater::UpdaterExt as _;
    // Prefer staged bytes for the same version (no re-download).
    let staged_path = {
        let staged = state.pending_update.lock().await;
        staged.clone().map(|s| (s.version, s.path))
    };
    let update = app
        .updater()
        .map_err(|err| IpcError::from(TalosError::Internal(format!("updater unavailable: {err}"))))?
        .check()
        .await
        .map_err(|err| IpcError::from(TalosError::Internal(format!("update check failed: {err}"))))?
        .ok_or_else(|| IpcError::from(TalosError::Storage("no update available".to_owned())))?;
    if let Some((staged_version, path)) = staged_path {
        if staged_version == update.version {
            if let Ok(bytes) = std::fs::read(&path) {
                update.install(bytes).map_err(|err| {
                    IpcError::from(TalosError::Internal(format!(
                        "update install failed: {err}"
                    )))
                })?;
                let _ = std::fs::remove_file(&path);
                *state.pending_update.lock().await = None;
                app.restart();
            }
        }
    }
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

/// `verify_update_manifest` — offline release-pipeline validation: parse a
/// release manifest and verify its minisign signature against a public key.
///
/// Release automation calls this before publishing a feed, so a malformed or
/// mis-signed manifest never reaches the staged-rollout endpoint. The
/// desktop's own install path is guarded separately by the plugin's
/// bundle-signature check.
#[tauri::command]
pub async fn verify_update_manifest(
    manifest_json: String,
    signature: String,
    pubkey: String,
) -> Result<talos_updates::UpdateManifest, IpcError> {
    let manifest = talos_updates::parse_update_manifest(&manifest_json)
        .map_err(|err| IpcError::from(TalosError::Storage(err.to_string())))?;
    talos_updates::verify_manifest_signature(manifest_json.as_bytes(), &signature, &pubkey)
        .map_err(|err| IpcError::from(TalosError::Policy(err.to_string())))?;
    Ok(manifest)
}

/// `get_update_channel` — read the persisted release channel.
#[tauri::command]
pub async fn get_update_channel(state: tauri::State<'_, AppState>) -> Result<String, IpcError> {
    Ok(read_channel(&state.db).await)
}

/// `set_update_channel` — persist the release channel (`stable` | `beta` |
/// `nightly`). Unknown values fall back to stable; beta/nightly builds ship
/// with their own endpoint, so a restart applies the switch.
#[tauri::command]
pub async fn set_update_channel(
    state: tauri::State<'_, AppState>,
    channel: String,
) -> Result<String, IpcError> {
    let next = UpdateChannel::parse(&channel);
    evidence_store::set_app_state(
        &state.db,
        evidence_store::app_state::keys::UPDATE_CHANNEL,
        next.as_str(),
    )
    .await
    .map_err(|err| IpcError::from(TalosError::Storage(err.to_string())))?;
    Ok(next.as_str().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_info_view_serializes_camel_case() {
        let view = UpdateInfoView {
            available: true,
            current_version: "0.4.0".to_owned(),
            latest_version: Some("0.5.0".to_owned()),
            notes: Some("hardening".to_owned()),
            pub_date: Some("2026-11-02T00:00:00Z".to_owned()),
            download_url: Some("https://updates.talos.dev/x".to_owned()),
        };
        let json = serde_json::to_value(&view).expect("serializes");
        assert_eq!(json["available"], true);
        assert_eq!(json["latestVersion"], "0.5.0");
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

    #[test]
    fn update_status_view_serializes_camel_case() {
        let view = UpdateStatusView {
            info: UpdateInfoView {
                available: true,
                current_version: "0.4.0".to_owned(),
                latest_version: Some("0.5.0".to_owned()),
                notes: None,
                pub_date: None,
                download_url: None,
            },
            channel: "beta".to_owned(),
            rollout_applicable: true,
            staged: true,
            staged_version: Some("0.5.0".to_owned()),
            staged_downloaded_at: Some("2026-11-02T00:00:00Z".to_owned()),
        };
        let json = serde_json::to_value(&view).expect("serializes");
        assert_eq!(json["channel"], "beta");
        assert_eq!(json["stagedVersion"], "0.5.0");
        assert_eq!(json["stagedDownloadedAt"], "2026-11-02T00:00:00Z");
        assert_eq!(json["rolloutApplicable"], true);
    }
}
