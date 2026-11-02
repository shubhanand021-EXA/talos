//! SQLite connection wrapper plus embedded migrations.

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous};
use talos_common::{now_iso, Result, TalosError};

use crate::crypto::{self, DatabaseKey};

/// Append-only evidence database handle.
///
/// The handle is cheap to clone (the pool is reference-counted). The optional
/// `path` records the backing file for file-backed databases; `None` means an
/// ephemeral in-memory database (tests only). When encryption is enabled,
/// `path` is the decrypted *working* file and `canonical_path` is the
/// ciphertext file at rest; `encryption_key` unlocks envelope operations
/// (backup encryption, envelope restore, shutdown re-encryption).
#[derive(Debug, Clone)]
pub struct EvidenceDb {
    pool: sqlx::SqlitePool,
    path: Option<PathBuf>,
    canonical_path: Option<PathBuf>,
    encryption_key: Option<DatabaseKey>,
}

impl EvidenceDb {
    /// Connect to `url` (e.g. `sqlite::memory:` or `sqlite:///path.db`)
    /// and run embedded migrations.
    ///
    /// Foreign keys are enforced pool-wide so link tables (`claim_evidence`,
    /// plan steps, evaluations) cannot reference missing parents.
    pub async fn connect(url: &str) -> Result<Self> {
        let options = SqliteConnectOptions::from_str(url)
            .map_err(|err| TalosError::Storage(err.to_string()))?
            .foreign_keys(true);
        let pool = sqlx::SqlitePool::connect_with(options)
            .await
            .map_err(|err| TalosError::Storage(err.to_string()))?;
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .map_err(|err| TalosError::Storage(err.to_string()))?;
        Ok(Self {
            pool,
            path: None,
            canonical_path: None,
            encryption_key: None,
        })
    }

    /// Connect to an isolated in-memory database.
    ///
    /// Tests only: production must use [`EvidenceDb::connect_file`] so state
    /// survives process restarts. Anything written here vanishes on drop.
    pub async fn connect_memory() -> Result<Self> {
        Self::connect("sqlite::memory:").await
    }

    /// Connect to (or create) the durable file-backed database at `path`.
    ///
    /// Creates missing parent directories, enables WAL mode with foreign keys,
    /// a 5s busy timeout and `NORMAL` synchronous, then runs all embedded
    /// migrations idempotently and records them in `schema_version`.
    pub async fn connect_file(path: &Path) -> Result<Self> {
        if path.as_os_str().is_empty() {
            return Err(TalosError::Storage(
                "database path must not be empty".to_owned(),
            ));
        }
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|err| TalosError::Storage(format!("create db dir: {err}")))?;
            }
        }
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(Duration::from_secs(5));
        let pool = sqlx::SqlitePool::connect_with(options)
            .await
            .map_err(|err| TalosError::Storage(err.to_string()))?;
        let db = Self {
            pool,
            path: Some(path.to_owned()),
            canonical_path: None,
            encryption_key: None,
        };
        sqlx::migrate!("./migrations")
            .run(db.pool())
            .await
            .map_err(|err| TalosError::Storage(err.to_string()))?;
        db.record_schema_versions().await?;
        Ok(db)
    }

    /// Sibling working file for an encrypted database (`talos.db` →
    /// `talos.work.db` in the same directory, so atomic replaces and backup
    /// layout keep working).
    #[must_use]
    pub fn work_path_for(canonical: &Path) -> PathBuf {
        canonical.with_extension("work.db")
    }

    /// True when this handle operates on an encrypted database (its canonical
    /// file on disk is a ChaCha20-Poly1305 envelope).
    #[must_use]
    pub fn is_encrypted(&self) -> bool {
        self.encryption_key.is_some()
    }

    /// Canonical at-rest path for encrypted databases (`None` otherwise).
    #[must_use]
    pub fn canonical_path(&self) -> Option<&Path> {
        self.canonical_path.as_deref()
    }

    /// Open (or create + migrate) the file-backed database under envelope
    /// encryption with `key`.
    ///
    /// - Missing canonical file + no orphan working file → create fresh.
    /// - Plaintext SQLite canonical file → checkpoint it, encrypt in place
    ///   (migration), then open as envelope. A stale working file next to a
    ///   plaintext canonical cannot hold unique data, so it is quarantined,
    ///   never deleted.
    /// - Envelope canonical file → decrypt to the working file and open it.
    /// - Orphan working file (previous crash before clean shutdown) → adopt
    ///   it as the live database (it holds the latest state).
    /// - Anything else → refuse (fail closed, never delete user data).
    pub async fn connect_encrypted_file(canonical: &Path, key: &DatabaseKey) -> Result<Self> {
        if canonical.as_os_str().is_empty() {
            return Err(TalosError::Storage(
                "database path must not be empty".to_owned(),
            ));
        }
        if let Some(parent) = canonical.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|err| TalosError::Storage(format!("create db dir: {err}")))?;
            }
        }
        let work = Self::work_path_for(canonical);
        match crypto::classify_file(canonical)? {
            crypto::FileKind::SqlitePlaintext => {
                // A stale working file cannot derive from a plaintext
                // canonical: quarantine it, never delete user data.
                if work.is_file() {
                    let quarantine = work.with_extension(format!(
                        "work.orphan-{}",
                        chrono::Utc::now().format("%Y%m%dT%H%M%SZ")
                    ));
                    std::fs::rename(&work, &quarantine).map_err(|err| {
                        TalosError::Storage(format!("quarantine stale work file: {err}"))
                    })?;
                }
                // Checkpoint first: uncheckpointed WAL rows live outside the
                // main file and would be lost by a raw byte encryption.
                let plain = Self::connect_file(canonical).await?;
                sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
                    .execute(plain.pool())
                    .await
                    .map_err(|err| TalosError::Storage(format!("checkpoint: {err}")))?;
                drain_pool(plain.pool()).await?;
                drop(plain);
                crypto::encrypt_file_in_place(canonical, key)?;
                // Drop leftover plaintext sidecars (main file is now sealed).
                for suffix in ["-wal", "-shm", "-journal"] {
                    let sidecar = PathBuf::from(format!("{}{suffix}", canonical.display()));
                    let _ = crypto::wipe_file(&sidecar);
                }
            }
            crypto::FileKind::Missing | crypto::FileKind::Envelope => {}
            crypto::FileKind::Unknown => {
                return Err(TalosError::Storage(format!(
                    "refusing to open unrecognized database file: {}",
                    canonical.display()
                )));
            }
        }
        // Adopt an orphan working file left by a crash (latest state wins);
        // otherwise decrypt the canonical envelope over a fresh working copy.
        if !work.is_file() {
            match crypto::classify_file(canonical)? {
                crypto::FileKind::Envelope => {
                    crypto::decrypt_file_to(canonical, &work, key)?;
                }
                crypto::FileKind::Missing => {
                    // Fresh database: the working file is created below by
                    // connect_work_file; the canonical envelope lands on close.
                }
                crypto::FileKind::SqlitePlaintext | crypto::FileKind::Unknown => {
                    return Err(TalosError::Storage(format!(
                        "unexpected database state for {}",
                        canonical.display()
                    )));
                }
            }
        } else if crypto::classify_file(&work)? != crypto::FileKind::SqlitePlaintext {
            // A working file that is not a readable database can never be
            // adopted (e.g. a wiped-but-not-deleted remnant): rehydrate from
            // the sealed canonical copy rather than failing the startup.
            // Best-effort: the decrypt below overwrites any remnant anyway.
            let _ = crate::crypto::wipe_file(&work);
            crypto::decrypt_file_to(canonical, &work, key)?;
        }
        let pool_db = Self::connect_work_file(&work).await?;
        // The working file is guaranteed to exist now: lock it to
        // owner-only on Unix (restrict before open would fail on first run,
        // when SQLite has not created the file yet).
        crypto::restrict_file(&work)?;
        Ok(Self {
            pool: pool_db.pool,
            path: Some(work),
            canonical_path: Some(canonical.to_owned()),
            encryption_key: Some(key.clone()),
        })
    }

    /// Open a working file with the standard WAL + migration setup.
    async fn connect_work_file(work: &Path) -> Result<Self> {
        let options = SqliteConnectOptions::new()
            .filename(work)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(Duration::from_secs(5));
        let pool = sqlx::SqlitePool::connect_with(options)
            .await
            .map_err(|err| TalosError::Storage(err.to_string()))?;
        let db = Self {
            pool,
            path: Some(work.to_owned()),
            canonical_path: None,
            encryption_key: None,
        };
        sqlx::migrate!("./migrations")
            .run(db.pool())
            .await
            .map_err(|err| TalosError::Storage(err.to_string()))?;
        db.record_schema_versions().await?;
        Ok(db)
    }

    /// Clean shutdown for encrypted databases: checkpoint the WAL, close the
    /// pool, encrypt the working file over the canonical path atomically,
    /// wipe the working copy + sidecars, and return. Plaintext handles are
    /// a no-op success.
    pub async fn close_encrypted(&self) -> Result<()> {
        let (Some(canonical), Some(key)) =
            (self.canonical_path.as_deref(), self.encryption_key.as_ref())
        else {
            return Ok(());
        };
        let Some(work) = self.path.as_deref() else {
            return Ok(());
        };
        sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .execute(self.pool())
            .await
            .map_err(|err| TalosError::Storage(format!("checkpoint: {err}")))?;
        // Release every file handle first: Windows cannot rename or delete
        // files with open handles, and the pool may be shared by clones.
        // drain_pool waits for sqlx's async finalizer (close() alone only
        // starts closing).
        drain_pool(self.pool()).await?;
        let bytes = std::fs::read(work)
            .map_err(|err| TalosError::Storage(format!("read work db: {err}")))?;
        if crypto::classify_bytes(&bytes) != crypto::FileKind::SqlitePlaintext {
            return Err(TalosError::Storage(
                "working database is not a readable SQLite file; refusing to seal".to_owned(),
            ));
        }
        let envelope = crypto::encrypt_bytes(key, &bytes);
        crypto::restrict_file(work)?;
        let tmp = crypto::tmp_sibling(canonical);
        std::fs::write(&tmp, &envelope)
            .map_err(|err| TalosError::Storage(format!("write sealed db: {err}")))?;
        crypto::retry_locked(|| std::fs::rename(&tmp, canonical))
            .map_err(|err| TalosError::Storage(format!("atomic seal: {err}")))?;
        Self::wipe_work_files_checked(work)?;
        if work.is_file() {
            return Err(TalosError::Storage(format!(
                "working copy still present after wipe: {}",
                work.display()
            )));
        }
        Ok(())
    }

    /// Best-effort wipe of a working file and its sidecars (never fails).
    /// Wipe a working file and its sidecars, failing loudly when the main
    /// file cannot be removed (a leftover working copy must never pass
    /// silently — it would shadow the sealed canonical on next startup).
    fn wipe_work_files_checked(work: &Path) -> Result<()> {
        if let Err(err) = crate::crypto::wipe_file(work) {
            return Err(TalosError::Storage(format!(
                "wipe working copy {}: {err}",
                work.display()
            )));
        }
        for suffix in ["-wal", "-shm", "-journal"] {
            let sidecar = PathBuf::from(format!("{}{suffix}", work.display()));
            let _ = crate::crypto::wipe_file(&sidecar);
        }
        Ok(())
    }

    /// Backing file for this database, if file-backed (`None` for memory).
    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Current journal mode (proof hook: file-backed databases report `wal`).
    pub async fn journal_mode(&self) -> Result<String> {
        let mode: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(self.pool())
            .await
            .map_err(|err| TalosError::Storage(err.to_string()))?;
        Ok(mode)
    }

    /// Highest applied migration version from `schema_version` (0 when none).
    pub async fn schema_version(&self) -> Result<i64> {
        let version: Option<i64> = sqlx::query_scalar("SELECT MAX(version) FROM schema_version")
            .fetch_optional(self.pool())
            .await
            .map_err(|err| TalosError::Storage(err.to_string()))?;
        Ok(version.unwrap_or(0))
    }

    /// Ensure the `schema_version` ledger exists and mirror every migration
    /// recorded by `sqlx::migrate!` into it (idempotent).
    async fn record_schema_versions(&self) -> Result<()> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS schema_version (
                version    INTEGER PRIMARY KEY,
                applied_at TEXT NOT NULL
            )",
        )
        .execute(self.pool())
        .await
        .map_err(|err| TalosError::Storage(err.to_string()))?;
        let applied: Vec<i64> =
            sqlx::query_scalar("SELECT version FROM _sqlx_migrations ORDER BY version")
                .fetch_all(self.pool())
                .await
                .map_err(|err| TalosError::Storage(err.to_string()))?;
        let now = now_iso();
        for version in applied {
            sqlx::query("INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (?, ?)")
                .bind(version)
                .bind(&now)
                .execute(self.pool())
                .await
                .map_err(|err| TalosError::Storage(err.to_string()))?;
        }
        Ok(())
    }
}

/// Close a pool and wait until every connection is actually released.
///
/// sqlx's `close()` future resolves once closing *starts*; SQLite OS
/// handles are released asynchronously afterwards, and Windows file
/// operations fail until they are. Poll `size()` so renames and deletes
/// never race the finalizer.
///
/// On timeout the error reports `size()` and `num_idle()` so the next
/// occurrence is diagnosable instead of a bare count (a stuck non-idle
/// connection points at a leaked checkout; a stuck idle count points at
/// the pool reaper).
pub(crate) async fn drain_pool(pool: &sqlx::SqlitePool) -> Result<()> {
    pool.close().await;
    for _ in 0..200 {
        if pool.size() == 0 {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    Err(TalosError::Storage(format!(
        "connection pool did not drain ({} connection(s) remain, {} idle)",
        pool.size(),
        pool.num_idle()
    )))
}

impl EvidenceDb {
    /// Borrow the underlying pool (repository functions take `&self` instead).
    #[must_use]
    pub fn pool(&self) -> &sqlx::SqlitePool {
        &self.pool
    }

    /// Assemble a handle from an existing pool (crate-internal: read-only
    /// backup probes and tests).
    pub(crate) fn from_pool(pool: sqlx::SqlitePool, path: Option<PathBuf>) -> Self {
        Self {
            pool,
            path,
            canonical_path: None,
            encryption_key: None,
        }
    }

    /// Encryption key held by this handle, if any (backup/restore helpers).
    pub(crate) fn encryption_key(&self) -> Option<&crypto::DatabaseKey> {
        self.encryption_key.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{insert_evidence, list_recent_evidence, NewEvidence};

    fn key() -> crypto::DatabaseKey {
        crypto::DatabaseKey::generate()
    }

    async fn seed_evidence(db: &EvidenceDb, id: &str) {
        insert_evidence(
            db,
            NewEvidence {
                id: id.to_owned(),
                task_id: Some("task-1".to_owned()),
                agent_id: None,
                session_id: None,
                kind: "log".to_owned(),
                label: None,
                summary: Some("seed".to_owned()),
                uri: None,
                mime_type: None,
                payload: serde_json::json!({ "marker": "secret-table-probe-xyz" }),
            },
        )
        .await
        .expect("inserts");
    }

    #[tokio::test]
    async fn encrypted_file_hides_plaintext() {
        let dir = tempfile::tempdir().expect("temp dir");
        let canonical = dir.path().join("talos.db");
        let db = EvidenceDb::connect_encrypted_file(&canonical, &key())
            .await
            .expect("creates encrypted db");
        assert!(db.is_encrypted());
        seed_evidence(&db, "ev-1").await;
        db.close_encrypted().await.expect("seals");
        drop(db);
        let bytes = std::fs::read(&canonical).expect("reads canonical");
        assert_eq!(crate::classify_bytes(&bytes), crate::FileKind::Envelope);
        let text = String::from_utf8_lossy(&bytes);
        assert!(!text.contains("SQLite"), "no sqlite magic on disk");
        for needle in [
            "runtime_tasks",
            "evidence_records",
            "event_log",
            "secret-table-probe-xyz",
        ] {
            assert!(
                !text.contains(needle),
                "no plaintext table names or content on disk: {needle}"
            );
        }
        // Working copy was wiped on clean shutdown.
        assert!(!EvidenceDb::work_path_for(&canonical).exists());
    }

    #[tokio::test]
    async fn encrypted_reopen_preserves_data() {
        let dir = tempfile::tempdir().expect("temp dir");
        let canonical = dir.path().join("talos.db");
        let key = key();
        let db = EvidenceDb::connect_encrypted_file(&canonical, &key)
            .await
            .expect("creates");
        seed_evidence(&db, "ev-keep").await;
        db.close_encrypted().await.expect("seals");
        drop(db);
        let reopened = EvidenceDb::connect_encrypted_file(&canonical, &key)
            .await
            .expect("reopens");
        let rows = list_recent_evidence(&reopened, 10).await.expect("lists");
        assert!(rows.iter().any(|r| r.id == "ev-keep"));
        reopened.close_encrypted().await.expect("seals again");
    }

    #[tokio::test]
    async fn wrong_key_fails_closed() {
        let dir = tempfile::tempdir().expect("temp dir");
        let canonical = dir.path().join("talos.db");
        let db = EvidenceDb::connect_encrypted_file(&canonical, &key())
            .await
            .expect("creates");
        db.close_encrypted().await.expect("seals");
        drop(db);
        let err = EvidenceDb::connect_encrypted_file(&canonical, &key())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("authentication failed"));
    }

    #[tokio::test]
    async fn plaintext_database_migrates_in_place() {
        let dir = tempfile::tempdir().expect("temp dir");
        let canonical = dir.path().join("talos.db");
        // Legacy plaintext database.
        let plain = EvidenceDb::connect_file(&canonical)
            .await
            .expect("plaintext connects");
        assert!(!plain.is_encrypted());
        seed_evidence(&plain, "ev-legacy").await;
        crate::db::drain_pool(plain.pool()).await.expect("drains");
        drop(plain);
        assert_eq!(
            crate::classify_file(&canonical).expect("classifies"),
            crate::FileKind::SqlitePlaintext
        );
        // Migration: same key flow as production startup.
        let key = key();
        let db = EvidenceDb::connect_encrypted_file(&canonical, &key)
            .await
            .expect("migrates");
        let rows = list_recent_evidence(&db, 10).await.expect("lists");
        assert!(rows.iter().any(|r| r.id == "ev-legacy"));
        db.close_encrypted().await.expect("seals");
        drop(db);
        let bytes = std::fs::read(&canonical).expect("reads");
        assert_eq!(crate::classify_bytes(&bytes), crate::FileKind::Envelope);
        assert!(!String::from_utf8_lossy(&bytes).contains("runtime_tasks"));
    }

    #[tokio::test]
    async fn orphan_work_file_is_adopted_not_deleted() {
        let dir = tempfile::tempdir().expect("temp dir");
        let canonical = dir.path().join("talos.db");
        let key = key();
        // Simulate a crash: open encrypted, write, drop WITHOUT close.
        let db = EvidenceDb::connect_encrypted_file(&canonical, &key)
            .await
            .expect("creates");
        seed_evidence(&db, "ev-crash").await;
        assert!(EvidenceDb::work_path_for(&canonical).is_file());
        drop(db);
        assert!(!canonical.is_file(), "no sealed copy yet");
        // Next startup adopts the orphan working copy (latest state wins).
        let reopened = EvidenceDb::connect_encrypted_file(&canonical, &key)
            .await
            .expect("adopts orphan");
        let rows = list_recent_evidence(&reopened, 10).await.expect("lists");
        assert!(rows.iter().any(|r| r.id == "ev-crash"));
        reopened.close_encrypted().await.expect("seals");
        assert!(canonical.is_file());
    }

    #[tokio::test]
    async fn unknown_file_refuses_to_open() {
        let dir = tempfile::tempdir().expect("temp dir");
        let canonical = dir.path().join("talos.db");
        std::fs::write(&canonical, b"definitely not a database").expect("writes");
        assert!(EvidenceDb::connect_encrypted_file(&canonical, &key())
            .await
            .is_err());
    }
}
