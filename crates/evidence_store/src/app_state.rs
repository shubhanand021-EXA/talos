//! Shared key-value state for the distribution layer.
//!
//! The desktop shell and the headless CLI (`crates/talos_cli`) share one
//! file-backed database, so cross-surface flags live here instead of in
//! process-local config files:
//! - `onboarding_completed` — `"true"` once the first-run wizard finishes.
//! - `update_channel` — `"stable"` | `"beta"` | `"nightly"`.
//! - `cli.providers` / `cli.mcp_servers` — JSON registries managed by the CLI.
//!
//! Values are opaque JSON strings. Callers validate on read; nothing stored
//! here is ever executed.

use talos_common::{now_iso, Result, TalosError};

use crate::db::EvidenceDb;

/// Well-known keys in [`get_app_state`] / [`set_app_state`].
pub mod keys {
    /// `"true"` once the first-run onboarding wizard has completed.
    pub const ONBOARDING_COMPLETED: &str = "onboarding_completed";
    /// Update channel preference (`stable` | `beta` | `nightly`).
    pub const UPDATE_CHANNEL: &str = "update_channel";
    /// JSON map of CLI-managed LLM providers.
    pub const CLI_PROVIDERS: &str = "cli.providers";
    /// JSON map of CLI-managed MCP servers.
    pub const CLI_MCP_SERVERS: &str = "cli.mcp_servers";
    /// JSON map of CLI-managed generic config keys (`config get/set`).
    pub const CLI_CONFIG: &str = "cli.config";
}

/// Read one state value (`None` when never set).
pub async fn get_app_state(db: &EvidenceDb, key: &str) -> Result<Option<String>> {
    if key.trim().is_empty() || key.len() > 128 {
        return Err(TalosError::Storage(
            "app_state key must be 1..=128 chars".to_owned(),
        ));
    }
    let value: Option<String> = sqlx::query_scalar("SELECT value FROM app_state WHERE key = ?")
        .bind(key)
        .fetch_optional(db.pool())
        .await
        .map_err(|err| TalosError::Storage(err.to_string()))?;
    Ok(value)
}

/// Upsert one state value (insert or replace, single statement).
pub async fn set_app_state(db: &EvidenceDb, key: &str, value: &str) -> Result<()> {
    if key.trim().is_empty() || key.len() > 128 {
        return Err(TalosError::Storage(
            "app_state key must be 1..=128 chars".to_owned(),
        ));
    }
    if value.len() > 65536 {
        return Err(TalosError::Storage(
            "app_state value must be <= 64 KiB".to_owned(),
        ));
    }
    let now = now_iso();
    sqlx::query(
        "INSERT INTO app_state (key, value, updated_at) VALUES (?, ?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
    )
    .bind(key)
    .bind(value)
    .bind(&now)
    .execute(db.pool())
    .await
    .map_err(|err| TalosError::Storage(err.to_string()))?;
    Ok(())
}

/// True once the first-run onboarding wizard has completed.
pub async fn is_onboarding_completed(db: &EvidenceDb) -> Result<bool> {
    Ok(get_app_state(db, keys::ONBOARDING_COMPLETED)
        .await?
        .as_deref()
        == Some("true"))
}

/// Read a JSON object map stored under `key` (empty map when never set).
pub async fn read_state_map(
    db: &EvidenceDb,
    key: &str,
) -> Result<serde_json::Map<String, serde_json::Value>> {
    match get_app_state(db, key).await? {
        None => Ok(serde_json::Map::new()),
        Some(raw) => match serde_json::from_str::<serde_json::Value>(&raw) {
            Ok(serde_json::Value::Object(map)) => Ok(map),
            _ => Err(TalosError::Storage(format!(
                "app_state key '{key}' is not a JSON object"
            ))),
        },
    }
}

/// Replace a JSON object map stored under `key`.
pub async fn write_state_map(
    db: &EvidenceDb,
    key: &str,
    map: &serde_json::Map<String, serde_json::Value>,
) -> Result<()> {
    let raw = serde_json::Value::Object(map.clone());
    set_app_state(db, key, &raw.to_string()).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn onboarding_flag_round_trips() {
        let db = EvidenceDb::connect_memory().await.expect("connects");
        assert!(!is_onboarding_completed(&db).await.expect("reads"));
        set_app_state(&db, keys::ONBOARDING_COMPLETED, "true")
            .await
            .expect("writes");
        assert!(is_onboarding_completed(&db).await.expect("reads"));
    }

    #[tokio::test]
    async fn missing_key_reads_none() {
        let db = EvidenceDb::connect_memory().await.expect("connects");
        assert_eq!(get_app_state(&db, "never-set").await.expect("reads"), None);
        assert!(read_state_map(&db, "never-set")
            .await
            .expect("reads")
            .is_empty());
    }

    #[tokio::test]
    async fn state_map_round_trips() {
        let db = EvidenceDb::connect_memory().await.expect("connects");
        let mut map = serde_json::Map::new();
        map.insert(
            "openai".to_owned(),
            serde_json::json!({"model": "gpt-4o-mini"}),
        );
        write_state_map(&db, keys::CLI_PROVIDERS, &map)
            .await
            .expect("writes");
        let back = read_state_map(&db, keys::CLI_PROVIDERS)
            .await
            .expect("reads");
        assert_eq!(back, map);
    }

    #[tokio::test]
    async fn rejects_bad_keys_and_oversize_values() {
        let db = EvidenceDb::connect_memory().await.expect("connects");
        assert!(set_app_state(&db, "", "v").await.is_err());
        assert!(set_app_state(&db, &"k".repeat(129), "v").await.is_err());
        assert!(get_app_state(&db, "").await.is_err());
        assert!(set_app_state(&db, "k", &"v".repeat(65537)).await.is_err());
    }

    #[tokio::test]
    async fn rejects_non_object_map() {
        let db = EvidenceDb::connect_memory().await.expect("connects");
        set_app_state(&db, "k", "[1,2]").await.expect("writes");
        assert!(read_state_map(&db, "k").await.is_err());
    }
}
