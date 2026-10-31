//! SQLite connection wrapper plus embedded migrations.

use talos_common::{Result, TalosError};

/// Append-only evidence database handle.
#[derive(Debug, Clone)]
pub struct EvidenceDb {
    pool: sqlx::SqlitePool,
}

impl EvidenceDb {
    /// Connect to `url` (e.g. `sqlite::memory:` or `sqlite:///path.db`)
    /// and run embedded migrations.
    pub async fn connect(url: &str) -> Result<Self> {
        let pool = sqlx::SqlitePool::connect(url)
            .await
            .map_err(|err| TalosError::Storage(err.to_string()))?;
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .map_err(|err| TalosError::Storage(err.to_string()))?;
        Ok(Self { pool })
    }

    /// Connect to an isolated in-memory database (tests and selftest).
    pub async fn connect_memory() -> Result<Self> {
        Self::connect("sqlite::memory:").await
    }

    /// Borrow the underlying pool (repository functions take `&self` instead).
    #[must_use]
    pub fn pool(&self) -> &sqlx::SqlitePool {
        &self.pool
    }
}
