//! Durable storage layout, backup, and restore for the evidence database.
//!
//! Layout (created on demand, path logged at startup — never content):
//! - `<app_data>/talos.db` — the WAL-mode SQLite database.
//! - `<app_data>/backups/` — timestamped `VACUUM INTO` snapshots.
//! - `<app_data>/models/` — local embedding/model assets.
//!
//! Platform defaults mirror the Tauri `app_data_dir` for `com.talos.desktop`:
//! - Windows: `%APPDATA%/com.talos.desktop/`
//! - macOS: `~/Library/Application Support/com.talos.desktop/`
//! - Linux/other: `$XDG_DATA_HOME/com.talos.desktop/` or
//!   `~/.local/share/com.talos.desktop/`
//!
//! `TALOS_DATA_DIR` overrides the base for tests and portable installs.
//!
//! Restore is a transactional content copy from the backup into the live
//! database (same file, same pool, WAL intact): the backup is validated
//! (SQLite magic + readable + identical migration set), then every user table
//! is replaced inside one transaction on a single connection with foreign keys
//! deferred. This avoids the close/replace/reopen dance — `SqlitePool::close`
//! is permanent, so swapping files under a live pool would orphan every
//! existing handle.

use std::path::{Path, PathBuf};

use talos_common::{now_iso, Result, TalosError};

use crate::db::EvidenceDb;
use crate::ledger::verify_chain;

/// Product identifier used for the platform app-data directory.
pub const APP_DIR_NAME: &str = "com.talos.desktop";

/// Database file name inside the app-data directory.
pub const DB_FILE_NAME: &str = "talos.db";

/// How many backups [`EvidenceDb::prune_backups`] keeps by default.
pub const BACKUP_RETENTION: usize = 5;

/// Resolved durable-storage layout.
#[derive(Debug, Clone)]
pub struct StorageLayout {
    /// Base app-data directory (`TALOS_DATA_DIR` or the platform default).
    pub base: PathBuf,
    /// Live database file (`<base>/talos.db`).
    pub db_file: PathBuf,
    /// Backup directory (`<base>/backups/`).
    pub backups_dir: PathBuf,
    /// Local model assets (`<base>/models/`).
    pub models_dir: PathBuf,
}

/// One timestamped backup snapshot.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BackupInfo {
    /// File name (`talos-<timestamp>.db`).
    pub file_name: String,
    /// Full backup path.
    pub path: PathBuf,
    /// File size in bytes.
    pub size_bytes: u64,
    /// Creation timestamp (RFC 3339, from file metadata).
    pub created_at: String,
}

/// Outcome of [`EvidenceDb::restore`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RestoreReport {
    /// Backup the content was copied from.
    pub backup_path: PathBuf,
    /// User tables replaced inside the restore transaction.
    pub tables_copied: Vec<String>,
    /// Event-log chain verdict after the restore.
    pub chain_valid: bool,
    /// Links recomputed in the post-restore verification.
    pub chain_checked: i64,
    /// When the restore completed (RFC 3339).
    pub restored_at: String,
}

/// Resolve the app-data base directory (no filesystem writes).
///
/// `TALOS_DATA_DIR` wins when set (tests, portable installs); otherwise the
/// platform default for `com.talos.desktop` is used. Fails only when no home
/// directory can be determined.
pub fn resolve_app_data_dir() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("TALOS_DATA_DIR") {
        let dir = dir.trim();
        if dir.is_empty() {
            return Err(TalosError::Storage(
                "TALOS_DATA_DIR must not be empty".to_owned(),
            ));
        }
        return Ok(PathBuf::from(dir));
    }
    default_app_data_dir()
}

/// Platform default for `com.talos.desktop` (no `TALOS_DATA_DIR` override).
pub fn default_app_data_dir() -> Result<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let base = std::env::var("APPDATA").map_err(|_| {
            TalosError::Storage("APPDATA is not set; cannot locate app data".to_owned())
        })?;
        Ok(PathBuf::from(base).join(APP_DIR_NAME))
    }
    #[cfg(target_os = "macos")]
    {
        let home = home_dir()?;
        Ok(home
            .join("Library")
            .join("Application Support")
            .join(APP_DIR_NAME))
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
            if !xdg.trim().is_empty() {
                return Ok(PathBuf::from(xdg).join(APP_DIR_NAME));
            }
        }
        let home = home_dir()?;
        Ok(home.join(".local").join("share").join(APP_DIR_NAME))
    }
}

#[cfg(not(target_os = "windows"))]
fn home_dir() -> Result<PathBuf> {
    std::env::var("HOME")
        .map(PathBuf::from)
        .map_err(|_| TalosError::Storage("HOME is not set; cannot locate app data".to_owned()))
}

/// Create the durable-storage layout under `base` and return it.
///
/// Creates `base`, `backups/`, and `models/`. Logs nothing (the caller logs
/// the path at startup); returns paths only, never content.
pub fn ensure_layout(base: &Path) -> Result<StorageLayout> {
    if base.as_os_str().is_empty() {
        return Err(TalosError::Storage(
            "app data dir must not be empty".to_owned(),
        ));
    }
    let layout = StorageLayout {
        base: base.to_owned(),
        db_file: base.join(DB_FILE_NAME),
        backups_dir: base.join("backups"),
        models_dir: base.join("models"),
    };
    for dir in [&layout.base, &layout.backups_dir, &layout.models_dir] {
        std::fs::create_dir_all(dir)
            .map_err(|err| TalosError::Storage(format!("create dir {}: {err}", dir.display())))?;
    }
    Ok(layout)
}

/// Quote a path as a SQLite string literal (single quotes doubled).
fn sqlite_quote(path: &Path) -> Result<String> {
    let raw = path.to_str().ok_or_else(|| {
        TalosError::Storage("path is not valid UTF-8; refusing to quote".to_owned())
    })?;
    Ok(format!("'{}'", raw.replace('\'', "''")))
}

/// Quote an identifier (double quotes doubled).
fn ident_quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Timestamp slug safe for file names (`20261101T120000Z`, no colons).
fn backup_slug() -> String {
    chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string()
}

/// Migrations applied to `db` (from `_sqlx_migrations`), for schema-match checks.
async fn applied_migrations(db: &EvidenceDb) -> Result<Vec<i64>> {
    sqlx::query_scalar::<_, i64>("SELECT version FROM _sqlx_migrations ORDER BY version")
        .fetch_all(db.pool())
        .await
        .map_err(|err| TalosError::Storage(err.to_string()))
}

/// Metadata timestamp for a file (RFC 3339, falls back to now).
fn file_created_at(path: &Path) -> String {
    std::fs::metadata(path)
        .and_then(|m| m.created().or_else(|_| m.modified()))
        .ok()
        .map(|t| {
            chrono::DateTime::<chrono::Utc>::from(t)
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
        })
        .unwrap_or_else(now_iso)
}

impl EvidenceDb {
    /// Backups directory for this database (`<db parent>/backups/`).
    ///
    /// Memory databases have no backups directory.
    fn backups_dir(&self) -> Result<PathBuf> {
        let path = self.path().ok_or_else(|| {
            TalosError::Storage("in-memory database has no backups directory".to_owned())
        })?;
        let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
        Ok(parent
            .map(|p| p.join("backups"))
            .unwrap_or_else(|| PathBuf::from("backups")))
    }

    /// Consistent snapshot via `VACUUM INTO`.
    ///
    /// `path = None` writes `<db parent>/backups/talos-<timestamp>.db` and
    /// prunes older snapshots to [`BACKUP_RETENTION`]; an explicit path is
    /// used verbatim (parents created) with no pruning. Returns the snapshot.
    pub async fn backup(&self, path: Option<&Path>) -> Result<PathBuf> {
        let target = match path {
            Some(explicit) => {
                if let Some(parent) = explicit.parent() {
                    if !parent.as_os_str().is_empty() {
                        std::fs::create_dir_all(parent).map_err(|err| {
                            TalosError::Storage(format!("create backup dir: {err}"))
                        })?;
                    }
                }
                explicit.to_owned()
            }
            None => {
                let dir = self.backups_dir()?;
                std::fs::create_dir_all(&dir)
                    .map_err(|err| TalosError::Storage(format!("create backup dir: {err}")))?;
                dir.join(format!("talos-{}.db", backup_slug()))
            }
        };
        if target.exists() {
            std::fs::remove_file(&target)
                .map_err(|err| TalosError::Storage(format!("replace backup: {err}")))?;
        }
        let quoted = sqlite_quote(&target)?;
        sqlx::query(&format!("VACUUM INTO {quoted}"))
            .execute(self.pool())
            .await
            .map_err(|err| TalosError::Storage(format!("vacuum into backup: {err}")))?;
        if path.is_none() {
            let _pruned = self.prune_backups(BACKUP_RETENTION).await?;
        }
        Ok(target)
    }

    /// List backups newest-first (empty when the directory is missing).
    pub async fn list_backups(&self) -> Result<Vec<BackupInfo>> {
        let dir = self.backups_dir()?;
        list_backups_in(&dir)
    }

    /// Delete oldest snapshots, keeping the newest `keep` (returns removals).
    pub async fn prune_backups(&self, keep: usize) -> Result<usize> {
        let dir = self.backups_dir()?;
        prune_backups_in(&dir, keep)
    }

    /// Validate a backup file: exists, SQLite magic header, opens read-only.
    ///
    /// Returns the throwaway read handle for deeper checks by the caller.
    async fn open_backup(backup_path: &Path) -> Result<EvidenceDb> {
        if !backup_path.is_file() {
            return Err(TalosError::Storage(format!(
                "backup not found: {}",
                backup_path.display()
            )));
        }
        let header = std::fs::read(backup_path)
            .map_err(|err| TalosError::Storage(format!("read backup: {err}")))?;
        if header.len() < 16 || &header[..16] != b"SQLite format 3\0" {
            return Err(TalosError::Storage(format!(
                "not a SQLite database: {}",
                backup_path.display()
            )));
        }
        let options = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(backup_path)
            .read_only(true);
        let pool = sqlx::SqlitePool::connect_with(options)
            .await
            .map_err(|err| TalosError::Storage(format!("open backup: {err}")))?;
        // Read-only handles have no `path` (never used for backup targets).
        Ok(EvidenceDb::from_pool(pool, None))
    }

    /// Restore this database from a backup snapshot.
    ///
    /// Validates the backup (exists, SQLite magic, opens read-only, identical
    /// migration set — schema drift is refused), then replaces every user
    /// table inside one transaction and verifies the hash chain. The live
    /// file, pool, and WAL mode are untouched; only content changes.
    pub async fn restore(&self, backup_path: &Path) -> Result<RestoreReport> {
        if self.path().is_none() {
            return Err(TalosError::Storage(
                "cannot restore into an in-memory database".to_owned(),
            ));
        }
        let src = Self::open_backup(backup_path).await?;
        let live_versions = applied_migrations(self).await?;
        let backup_versions = applied_migrations(&src).await.map_err(|_| {
            TalosError::Storage("backup has no migration history; refusing restore".to_owned())
        })?;
        if live_versions != backup_versions {
            return Err(TalosError::Storage(format!(
                "backup schema mismatch (live {live_versions:?} vs backup {backup_versions:?}); refusing restore"
            )));
        }
        let tables: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .fetch_all(src.pool())
        .await
        .map_err(|err| TalosError::Storage(format!("list backup tables: {err}")))?;
        if tables.is_empty() {
            return Err(TalosError::Storage("backup contains no tables".to_owned()));
        }
        // One connection for the whole restore. FK enforcement is switched off
        // on this connection only (a no-op inside transactions, so it is set
        // before BEGIN) and restored to ON before the connection returns to
        // the pool. `BEGIN IMMEDIATE` serializes against concurrent writers.
        // DETACH runs after COMMIT: SQLite refuses to detach a database that
        // participates in the open write transaction.
        let mut conn = self
            .pool()
            .acquire()
            .await
            .map_err(|err| TalosError::Storage(err.to_string()))?;
        sqlx::query("PRAGMA foreign_keys=OFF")
            .execute(&mut *conn)
            .await
            .map_err(|err| TalosError::Storage(err.to_string()))?;
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *conn)
            .await
            .map_err(|err| TalosError::Storage(err.to_string()))?;
        let quoted = sqlite_quote(backup_path)?;
        let outcome: Result<()> = async {
            sqlx::query(&format!("ATTACH DATABASE {quoted} AS src"))
                .execute(&mut *conn)
                .await
                .map_err(|err| TalosError::Storage(format!("attach backup: {err}")))?;
            for table in &tables {
                let quoted_table = ident_quote(table);
                sqlx::query(&format!("DELETE FROM main.{quoted_table}"))
                    .execute(&mut *conn)
                    .await
                    .map_err(|err| TalosError::Storage(format!("clear {table}: {err}")))?;
                sqlx::query(&format!(
                    "INSERT INTO main.{quoted_table} SELECT * FROM src.{quoted_table}"
                ))
                .execute(&mut *conn)
                .await
                .map_err(|err| TalosError::Storage(format!("copy {table}: {err}")))?;
            }
            // `sqlite_sequence` holds AUTOINCREMENT counters; copy it too.
            sqlx::query("DELETE FROM main.sqlite_sequence")
                .execute(&mut *conn)
                .await
                .map_err(|err| TalosError::Storage(format!("clear sqlite_sequence: {err}")))?;
            sqlx::query("INSERT INTO main.sqlite_sequence SELECT * FROM src.sqlite_sequence")
                .execute(&mut *conn)
                .await
                .map_err(|err| TalosError::Storage(format!("copy sqlite_sequence: {err}")))?;
            sqlx::query("COMMIT")
                .execute(&mut *conn)
                .await
                .map_err(|err| TalosError::Storage(format!("commit restore: {err}")))?;
            Ok(())
        }
        .await;
        if outcome.is_err() {
            let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
        }
        // Always release the attach alias (best-effort after a failure) and
        // restore FK enforcement before the connection rejoins the pool.
        let detach_outcome = sqlx::query("DETACH DATABASE src")
            .execute(&mut *conn)
            .await
            .map_err(|err| TalosError::Storage(format!("detach backup: {err}")));
        sqlx::query("PRAGMA foreign_keys=ON")
            .execute(&mut *conn)
            .await
            .map_err(|err| TalosError::Storage(err.to_string()))?;
        outcome?;
        detach_outcome?;
        let report = verify_chain(self).await?;
        Ok(RestoreReport {
            backup_path: backup_path.to_owned(),
            tables_copied: tables,
            chain_valid: report.valid,
            chain_checked: report.checked,
            restored_at: now_iso(),
        })
    }

    /// Database file size in bytes (plus `-wal`/`-shm` via [`EvidenceDb::wal_size_bytes`]).
    pub fn db_size_bytes(&self) -> Result<u64> {
        let path = self
            .path()
            .ok_or_else(|| TalosError::Storage("in-memory database has no file size".to_owned()))?;
        std::fs::metadata(path)
            .map(|m| m.len())
            .map_err(|err| TalosError::Storage(format!("stat database: {err}")))
    }

    /// Sidecar (`-wal` + `-shm`) size in bytes, 0 when checkpointed away.
    pub fn wal_size_bytes(&self) -> Result<u64> {
        let path = self
            .path()
            .ok_or_else(|| TalosError::Storage("in-memory database has no WAL files".to_owned()))?;
        let mut total = 0_u64;
        for suffix in ["-wal", "-shm"] {
            let sidecar = PathBuf::from(format!("{}{suffix}", path.display()));
            if let Ok(meta) = std::fs::metadata(&sidecar) {
                total += meta.len();
            }
        }
        Ok(total)
    }
}

/// List backups in an explicit directory newest-first (path-agnostic helper).
pub fn list_backups_in(dir: &Path) -> Result<Vec<BackupInfo>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(TalosError::Storage(format!("list backups: {err}"))),
    };
    let mut backups = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|err| TalosError::Storage(format!("list backups: {err}")))?;
        let path = entry.path();
        let is_db = path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("db"));
        if !is_db || !path.is_file() {
            continue;
        }
        let size_bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
        backups.push(BackupInfo {
            file_name: path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_owned(),
            path,
            size_bytes,
            created_at: String::new(),
        });
    }
    for backup in &mut backups {
        backup.created_at = file_created_at(&backup.path);
    }
    backups.sort_by(|a, b| b.file_name.cmp(&a.file_name));
    Ok(backups)
}

/// Delete oldest snapshots in `dir`, keeping the newest `keep`.
pub fn prune_backups_in(dir: &Path, keep: usize) -> Result<usize> {
    let backups = list_backups_in(dir)?;
    let mut removed = 0_usize;
    for stale in backups.iter().skip(keep) {
        match std::fs::remove_file(&stale.path) {
            Ok(()) => removed += 1,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(TalosError::Storage(format!(
                    "prune backup {}: {err}",
                    stale.path.display()
                )))
            }
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::append_event;
    use crate::{NewEvent, NewEvidence};

    fn test_layout(name: &str) -> (tempfile::TempDir, StorageLayout) {
        let dir = tempfile::tempdir().expect("temp dir");
        let base = dir.path().join(name);
        let layout = ensure_layout(&base).expect("layout");
        (dir, layout)
    }

    async fn file_db(layout: &StorageLayout) -> EvidenceDb {
        EvidenceDb::connect_file(&layout.db_file)
            .await
            .expect("file database connects")
    }

    #[test]
    fn resolves_override_dir_without_touching_fs() {
        let dir = tempfile::tempdir().expect("temp dir");
        let custom = dir.path().join("custom-data");
        let prior = std::env::var("TALOS_DATA_DIR").ok();
        std::env::set_var("TALOS_DATA_DIR", &custom);
        let resolved = resolve_app_data_dir().expect("resolves");
        assert_eq!(resolved, custom);
        assert!(!custom.exists(), "resolution must not create directories");
        match prior {
            Some(value) => std::env::set_var("TALOS_DATA_DIR", value),
            None => std::env::remove_var("TALOS_DATA_DIR"),
        }
    }

    #[test]
    fn layout_creates_base_backups_and_models() {
        let (_dir, layout) = test_layout("layout");
        assert!(layout.base.is_dir());
        assert!(layout.backups_dir.is_dir());
        assert!(layout.models_dir.is_dir());
        assert_eq!(layout.db_file.file_name().unwrap(), DB_FILE_NAME);
    }

    #[tokio::test]
    async fn connect_file_enables_wal_and_records_schema() {
        let (_dir, layout) = test_layout("wal");
        let db = file_db(&layout).await;
        assert_eq!(db.journal_mode().await.expect("journal mode"), "wal");
        assert!(layout.db_file.is_file());
        // 7 embedded migrations mirrored into schema_version.
        assert_eq!(db.schema_version().await.expect("schema version"), 7);
        // Idempotent: reconnecting an existing database succeeds unchanged.
        drop(db);
        let reopened = file_db(&layout).await;
        assert_eq!(reopened.schema_version().await.expect("schema version"), 7);
    }

    #[tokio::test]
    async fn migrations_are_idempotent_no_duplicate_rows() {
        let (_dir, layout) = test_layout("idempotent");
        let db = file_db(&layout).await;
        append_event(
            &db,
            NewEvent {
                id: "evt-1".to_owned(),
                kind: "task_created".to_owned(),
                payload_json: serde_json::json!({}),
            },
        )
        .await
        .expect("append works");
        drop(db);
        let db = file_db(&layout).await;
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM event_log")
            .fetch_one(db.pool())
            .await
            .expect("counts");
        assert_eq!(count, 1);
        let versions: Vec<i64> =
            sqlx::query_scalar("SELECT version FROM schema_version ORDER BY version")
                .fetch_all(db.pool())
                .await
                .expect("lists versions");
        assert_eq!(versions, vec![1, 2, 3, 4, 5, 6, 7]);
    }

    #[tokio::test]
    async fn data_survives_close_and_reopen() {
        let (_dir, layout) = test_layout("survives");
        let db = file_db(&layout).await;
        append_event(
            &db,
            NewEvent {
                id: "evt-persist".to_owned(),
                kind: "task_created".to_owned(),
                payload_json: serde_json::json!({ "task": "t-1" }),
            },
        )
        .await
        .expect("append works");
        crate::insert_evidence(
            &db,
            NewEvidence {
                id: "ev-persist".to_owned(),
                task_id: None,
                agent_id: None,
                session_id: None,
                kind: "log".to_owned(),
                label: None,
                summary: None,
                uri: None,
                mime_type: None,
                payload: serde_json::json!({ "ok": true }),
            },
        )
        .await
        .expect("insert works");
        drop(db);
        let db = file_db(&layout).await;
        let chain = verify_chain(&db).await.expect("verifies");
        assert!(chain.valid);
        assert_eq!(chain.checked, 1);
        let rows = crate::list_recent_evidence(&db, 10).await.expect("lists");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "ev-persist");
    }

    #[tokio::test]
    async fn backup_restore_round_trip() {
        let (_dir, layout) = test_layout("roundtrip");
        let db = file_db(&layout).await;
        append_event(
            &db,
            NewEvent {
                id: "evt-before".to_owned(),
                kind: "task_created".to_owned(),
                payload_json: serde_json::json!({}),
            },
        )
        .await
        .expect("append works");
        let snapshot = db.backup(None).await.expect("backup works");
        assert!(snapshot.is_file());
        // Diverge the live database after the snapshot.
        append_event(
            &db,
            NewEvent {
                id: "evt-after".to_owned(),
                kind: "task_created".to_owned(),
                payload_json: serde_json::json!({}),
            },
        )
        .await
        .expect("append works");
        let report = db.restore(&snapshot).await.expect("restore works");
        assert!(report.chain_valid);
        assert!(report.tables_copied.contains(&"event_log".to_owned()));
        let rows = crate::list_recent_log(&db, 10).await.expect("lists");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "evt-before");
    }

    #[tokio::test]
    async fn restore_rejects_non_sqlite_files() {
        let (_dir, layout) = test_layout("reject");
        let db = file_db(&layout).await;
        let bogus = layout.backups_dir.join("bogus.db");
        std::fs::write(&bogus, b"definitely not sqlite").expect("writes");
        assert!(db.restore(&bogus).await.is_err());
        assert!(db
            .restore(&layout.backups_dir.join("missing.db"))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn retention_keeps_newest_five() {
        let (_dir, layout) = test_layout("retention");
        let db = file_db(&layout).await;
        for i in 0..7 {
            let target = layout.backups_dir.join(format!("talos-manual-{i:02}.db"));
            db.backup(Some(&target)).await.expect("backup works");
        }
        assert_eq!(db.list_backups().await.expect("lists").len(), 7);
        let removed = db.prune_backups(5).await.expect("prunes");
        assert_eq!(removed, 2);
        let remaining = db.list_backups().await.expect("lists");
        assert_eq!(remaining.len(), 5);
        assert!(remaining.iter().all(|b| b.size_bytes > 0));
    }

    #[tokio::test]
    async fn concurrent_file_connections_do_not_corrupt() {
        let (_dir, layout) = test_layout("concurrent");
        let first = file_db(&layout).await;
        let second = file_db(&layout).await;
        let mut handles = Vec::new();
        for writer in 0..4 {
            let db = if writer % 2 == 0 {
                first.clone()
            } else {
                second.clone()
            };
            handles.push(tokio::spawn(async move {
                for i in 0..10 {
                    append_event(
                        &db,
                        NewEvent {
                            id: format!("evt-w{writer}-{i}"),
                            kind: "task_created".to_owned(),
                            payload_json: serde_json::json!({}),
                        },
                    )
                    .await
                    .expect("append works");
                }
            }));
        }
        for handle in handles {
            handle.await.expect("writer joins");
        }
        let chain = verify_chain(&first).await.expect("verifies");
        assert!(chain.valid);
        assert_eq!(chain.checked, 40);
    }
}
