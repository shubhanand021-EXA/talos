//! SQLite connection wrapper plus embedded migrations.

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous};
use talos_common::{now_iso, Result, TalosError};

/// Append-only evidence database handle.
///
/// The handle is cheap to clone (the pool is reference-counted). The optional
/// `path` records the backing file for file-backed databases; `None` means an
/// ephemeral in-memory database (tests only).
#[derive(Debug, Clone)]
pub struct EvidenceDb {
    pool: sqlx::SqlitePool,
    path: Option<PathBuf>,
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
        Ok(Self { pool, path: None })
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
        };
        sqlx::migrate!("./migrations")
            .run(db.pool())
            .await
            .map_err(|err| TalosError::Storage(err.to_string()))?;
        db.record_schema_versions().await?;
        Ok(db)
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

    /// Borrow the underlying pool (repository functions take `&self` instead).
    #[must_use]
    pub fn pool(&self) -> &sqlx::SqlitePool {
        &self.pool
    }

    /// Assemble a handle from an existing pool (crate-internal: read-only
    /// backup probes and tests).
    pub(crate) fn from_pool(pool: sqlx::SqlitePool, path: Option<PathBuf>) -> Self {
        Self { pool, path }
    }
}
