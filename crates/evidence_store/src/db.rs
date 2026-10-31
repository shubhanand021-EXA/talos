//! SQLite connection wrapper plus embedded migrations.

use std::str::FromStr;

use sqlx::sqlite::SqliteConnectOptions;
use talos_common::{Result, TalosError};

/// Append-only evidence database handle.
#[derive(Debug, Clone)]
pub struct EvidenceDb {
    pool: sqlx::SqlitePool,
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
