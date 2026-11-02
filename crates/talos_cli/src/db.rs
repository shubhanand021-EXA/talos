//! Shared database handle for the CLI.
//!
//! The CLI and the desktop app share the same file-backed `EvidenceDb`
//! (resolved exactly like the desktop: `TALOS_DATA_DIR` override or the
//! platform `com.talos.desktop` app-data dir). When the desktop runs with
//! at-rest encryption, the CLI reuses the OS-keychain database key through
//! the same [`evidence_store::load_or_create_key`] path, so both surfaces
//! read and write one ledger.
//!
//! Concurrency note: SQLite WAL mode supports concurrent desktop + CLI
//! access, but the encrypted working-copy handshake assumes no competing
//! sealer. Prefer running CLI writes while the desktop is closed, or point
//! `TALOS_DATA_DIR` at a scratch copy for fully isolated runs.

use evidence_store::EvidenceDb;
use talos_common::{Result, TalosError};

/// Opened database plus metadata the commands surface in `doctor`.
#[derive(Debug)]
pub struct CliDb {
    /// Shared evidence database handle.
    pub db: EvidenceDb,
    /// Resolved app-data directory.
    pub app_data_dir: std::path::PathBuf,
    /// Live database file.
    pub db_file: std::path::PathBuf,
    /// Whether at-rest encryption is active.
    pub encrypted: bool,
}

/// Open the shared database (creating directories as needed).
///
/// `TALOS_DATA_DIR` forces an isolated database (tests, portable installs,
/// CI) — otherwise the platform default shared with the desktop is used.
pub async fn open_shared_db() -> Result<CliDb> {
    let base = evidence_store::storage::resolve_app_data_dir()?;
    let layout = evidence_store::storage::ensure_layout(&base)?;
    // Same key path as the desktop: keychain-held 256-bit key when
    // available, loud plaintext fallback otherwise (never silent).
    let vault = talos_secrets::SecretVault::os();
    let (db, encrypted) = match evidence_store::load_or_create_key(&vault) {
        Ok(evidence_store::KeyState::Encrypted(key)) => {
            match EvidenceDb::connect_encrypted_file(&layout.db_file, &key).await {
                Ok(db) => (db, true),
                Err(err) => {
                    return Err(TalosError::Storage(format!(
                        "shared database is sealed and the CLI cannot unlock it \
                         (is the desktop mid-shutdown?): {err}"
                    )));
                }
            }
        }
        Ok(evidence_store::KeyState::Unavailable(_)) => {
            (EvidenceDb::connect_file(&layout.db_file).await?, false)
        }
        Err(err) => return Err(err),
    };
    Ok(CliDb {
        db,
        app_data_dir: layout.base,
        db_file: layout.db_file,
        encrypted,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn opens_isolated_db_under_talos_data_dir() {
        let _guard = crate::TEST_ENV_LOCK.lock().await;
        let dir = tempfile::tempdir().expect("temp dir");
        std::env::set_var("TALOS_DATA_DIR", dir.path());
        let cli = open_shared_db().await.expect("opens");
        assert!(cli.db_file.starts_with(dir.path()));
        std::env::remove_var("TALOS_DATA_DIR");
    }
}
