//! Durable-storage IPC: info, backup, restore, and integrity checks.
//!
//! All commands require file-backed state ([`AppState::new_persistent`]); on
//! in-memory test state they fail with `storage_unavailable`. Paths are
//! surfaced read-only; content never leaves the ledger except through the
//! existing evidence IPC.

use talos_common::TalosError;

use crate::commands::IpcError;
use crate::events::{emit_talos_event, BACKUP_CREATED, BACKUP_RESTORED, INTEGRITY_CHECK_COMPLETED};
use crate::state::AppState;

/// Storage health snapshot (camelCase JSON for the Settings panel).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageInfo {
    /// Live database file path.
    pub db_path: String,
    /// `talos.db` size in bytes.
    pub db_size_bytes: u64,
    /// `-wal` + `-shm` sidecar size in bytes.
    pub wal_size_bytes: u64,
    /// Highest applied migration version.
    pub schema_version: i64,
    /// Newest-first backup count.
    pub backup_count: usize,
    /// RFC 3339 creation time of the newest backup, if any.
    pub last_backup_at: Option<String>,
    /// Event-log chain verdict from the last check (startup recovery).
    pub integrity_valid: bool,
    /// Links recomputed in the last check.
    pub chain_checked: i64,
    /// Highest verified sequence, if any.
    pub chain_last_seq: Option<i64>,
    /// When the last integrity check ran (RFC 3339).
    pub last_checked_at: String,
}

/// One backup snapshot (camelCase JSON).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupView {
    /// File name (`talos-<timestamp>.db`).
    pub file_name: String,
    /// Full backup path.
    pub path: String,
    /// File size in bytes.
    pub size_bytes: u64,
    /// Creation timestamp (RFC 3339).
    pub created_at: String,
}

/// Restore outcome (camelCase JSON).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreView {
    /// Backup the content was copied from.
    pub backup_path: String,
    /// User tables replaced in the restore transaction.
    pub tables_copied: Vec<String>,
    /// Event-log chain verdict after the restore.
    pub chain_valid: bool,
    /// Links recomputed in the post-restore verification.
    pub chain_checked: i64,
    /// When the restore completed (RFC 3339).
    pub restored_at: String,
}

fn backup_view(info: &evidence_store::BackupInfo) -> BackupView {
    BackupView {
        file_name: info.file_name.clone(),
        path: info.path.display().to_string(),
        size_bytes: info.size_bytes,
        created_at: info.created_at.clone(),
    }
}

/// Require file-backed state; fail loudly on in-memory test state.
fn persistent_paths(state: &AppState) -> Result<(&std::path::Path, &std::path::Path), IpcError> {
    match (&state.db_path, &state.backups_dir) {
        (Some(db), Some(backups)) => Ok((db.as_path(), backups.as_path())),
        _ => Err(IpcError::from(TalosError::Storage(
            "persistence unavailable: state is in-memory (tests only)".to_owned(),
        ))),
    }
}

/// Pure storage-info snapshot (unit-tested without a Tauri handle).
pub async fn storage_info_of(state: &AppState) -> Result<StorageInfo, IpcError> {
    let (db_path, _) = persistent_paths(state)?;
    let backups = state.db.list_backups().await?;
    let recovery = state.startup_recovery.as_ref();
    Ok(StorageInfo {
        db_path: db_path.display().to_string(),
        db_size_bytes: state.db.db_size_bytes()?,
        wal_size_bytes: state.db.wal_size_bytes()?,
        schema_version: state.db.schema_version().await?,
        backup_count: backups.len(),
        last_backup_at: backups.first().map(|b| b.created_at.clone()),
        integrity_valid: recovery.map(|r| r.chain_valid).unwrap_or(true),
        chain_checked: recovery.map(|r| r.chain_checked).unwrap_or(0),
        chain_last_seq: None,
        last_checked_at: recovery
            .map(|r| r.recovered_at.clone())
            .unwrap_or_else(talos_common::now_iso),
    })
}

/// `get_storage_info` — database path, sizes, schema, backups, integrity.
#[tauri::command]
pub async fn get_storage_info(state: tauri::State<'_, AppState>) -> Result<StorageInfo, IpcError> {
    storage_info_of(&state).await
}

/// `create_backup` — snapshot via `VACUUM INTO`, prune to retention, emit.
#[tauri::command]
pub async fn create_backup(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<BackupView, IpcError> {
    persistent_paths(&state)?;
    let path = state.db.backup(None).await?;
    let backups = state.db.list_backups().await?;
    let view = backups
        .iter()
        .find(|b| b.path == path)
        .map(backup_view)
        .unwrap_or_else(|| BackupView {
            file_name: path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_owned(),
            path: path.display().to_string(),
            size_bytes: 0,
            created_at: talos_common::now_iso(),
        });
    let payload = serde_json::json!({
        "path": view.path,
        "sizeBytes": view.size_bytes,
        "backupCount": backups.len(),
        "checkedAt": talos_common::now_iso(),
    });
    if let Err(err) = emit_talos_event(&app, BACKUP_CREATED, payload) {
        eprintln!("[talos] event emission failed ({BACKUP_CREATED}): {err}");
    }
    Ok(view)
}

/// `list_backups` — newest-first snapshots.
#[tauri::command]
pub async fn list_backups(state: tauri::State<'_, AppState>) -> Result<Vec<BackupView>, IpcError> {
    persistent_paths(&state)?;
    Ok(state
        .db
        .list_backups()
        .await?
        .iter()
        .map(backup_view)
        .collect())
}

/// `restore_backup` — transactional content copy from a snapshot, then verify.
#[tauri::command]
pub async fn restore_backup(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    backup_path: String,
) -> Result<RestoreView, IpcError> {
    persistent_paths(&state)?;
    if backup_path.trim().is_empty() || backup_path.len() > 1024 {
        return Err(IpcError::from(TalosError::Storage(
            "backup_path must be 1..=1024 chars".to_owned(),
        )));
    }
    let report = state.db.restore(std::path::Path::new(&backup_path)).await?;
    let view = RestoreView {
        backup_path: report.backup_path.display().to_string(),
        tables_copied: report.tables_copied.clone(),
        chain_valid: report.chain_valid,
        chain_checked: report.chain_checked,
        restored_at: report.restored_at.clone(),
    };
    let restored_payload = serde_json::json!({
        "path": view.backup_path,
        "tablesCopied": view.tables_copied,
        "chainValid": view.chain_valid,
        "chainChecked": view.chain_checked,
        "checkedAt": talos_common::now_iso(),
    });
    if let Err(err) = emit_talos_event(&app, BACKUP_RESTORED, restored_payload) {
        eprintln!("[talos] event emission failed ({BACKUP_RESTORED}): {err}");
    }
    let integrity_payload = serde_json::json!({
        "valid": view.chain_valid,
        "checked": view.chain_checked,
        "firstBadSeq": null,
        "checkedAt": view.restored_at,
    });
    if let Err(err) = emit_talos_event(&app, INTEGRITY_CHECK_COMPLETED, integrity_payload) {
        eprintln!("[talos] event emission failed ({INTEGRITY_CHECK_COMPLETED}): {err}");
    }
    Ok(view)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn persistent_state() -> AppState {
        let dir = tempfile::tempdir().expect("temp dir");
        // Leak the temp dir for the lifetime of the test state: the database
        // file must outlive the test body while the pool is open. The OS
        // reclaims temp contents; leaking here only affects test processes.
        let base = Box::leak(dir.path().join("talos-data").into_boxed_path());
        std::mem::forget(dir);
        AppState::new_persistent_with_vault(base, talos_secrets::SecretVault::mock())
            .await
            .expect("persistent state builds")
    }

    #[tokio::test]
    async fn storage_info_reports_file_backed_database() {
        let state = persistent_state().await;
        let info = storage_info_of(&state).await.expect("info works");
        assert!(info.db_path.ends_with("talos.db"));
        assert!(std::path::Path::new(&info.db_path).is_file());
        assert!(info.schema_version >= 7);
        assert!(info.integrity_valid);
        assert_eq!(info.backup_count, 0);
    }

    #[tokio::test]
    async fn in_memory_state_refuses_storage_commands() {
        let state = AppState::new_in_memory_with_vault(talos_secrets::SecretVault::mock())
            .await
            .expect("memory state builds");
        assert!(storage_info_of(&state).await.is_err());
        assert!(state.db.list_backups().await.is_err());
    }

    #[tokio::test]
    async fn recovery_terminal_set_matches_lifecycle() {
        // `evidence_store` mirrors the lifecycle terminal set (it cannot
        // depend on `agent_runtime`); this test keeps the mirror honest.
        for status in evidence_store::TERMINAL_STATUSES {
            assert!(
                agent_runtime::lifecycle::is_terminal(status),
                "{status} must be terminal in agent_runtime"
            );
        }
        for status in agent_runtime::lifecycle::STATES {
            assert_eq!(
                agent_runtime::lifecycle::is_terminal(status),
                evidence_store::is_terminal_status(status),
                "terminal verdict must agree for {status}"
            );
        }
    }
}
