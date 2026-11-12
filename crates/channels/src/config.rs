//! Channel configuration: `.talos/channels.json` holds enablement +
//! allowlists only. Secrets (bot tokens, webhook URLs) live in the OS
//! keychain under `channel.<name>.<key>` and are resolved at runtime —
//! they are never written to disk, logged, or returned to clients.

use std::path::Path;

use talos_common::{Result, TalosError};

/// Config file name inside the app-data dir.
pub const CHANNELS_FILE_NAME: &str = "channels.json";

/// Keychain key for the Telegram bot token.
pub const TELEGRAM_TOKEN_KEY: &str = "channel.telegram.token";
/// Keychain key for the Discord bot token.
pub const DISCORD_TOKEN_KEY: &str = "channel.discord.token";
/// Keychain key for the Discord incoming-webhook URL.
pub const DISCORD_WEBHOOK_KEY: &str = "channel.discord.webhook";

/// Telegram surface config (no secrets).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TelegramChannelConfig {
    /// Whether the adapter polls.
    #[serde(default)]
    pub enabled: bool,
    /// Allowed Telegram chat ids (empty ⇒ deny all).
    #[serde(default)]
    pub allowed_chat_ids: Vec<String>,
    /// Long-poll cadence hint in seconds (daemon loop is authoritative).
    #[serde(default = "default_poll_secs")]
    pub poll_interval_secs: u64,
}

/// Discord surface config (no secrets).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DiscordChannelConfig {
    /// Whether the adapter polls.
    #[serde(default)]
    pub enabled: bool,
    /// Allowed Discord channel ids for receives (empty ⇒ deny all).
    #[serde(default)]
    pub allowed_channel_ids: Vec<String>,
    /// Channel id used for bot-token sends when set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_id: Option<String>,
    /// Long-poll cadence hint in seconds (daemon loop is authoritative).
    #[serde(default = "default_poll_secs")]
    pub poll_interval_secs: u64,
}

fn default_poll_secs() -> u64 {
    30
}

/// Whole-file config.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ChannelConfigs {
    /// Telegram surface.
    #[serde(default)]
    pub telegram: TelegramChannelConfig,
    /// Discord surface.
    #[serde(default)]
    pub discord: DiscordChannelConfig,
}

impl Default for TelegramChannelConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            allowed_chat_ids: Vec::new(),
            poll_interval_secs: 30,
        }
    }
}

impl Default for DiscordChannelConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            allowed_channel_ids: Vec::new(),
            channel_id: None,
            poll_interval_secs: 30,
        }
    }
}

impl ChannelConfigs {
    /// Load from `dir/channels.json`; missing or corrupt ⇒ disabled default
    /// (fail closed, never crash the daemon).
    #[must_use]
    pub fn load_or_default(dir: &Path) -> Self {
        let path = dir.join(CHANNELS_FILE_NAME);
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        serde_json::from_str(&text).unwrap_or_default()
    }

    /// Persist the (secret-free) config.
    pub fn save(&self, dir: &Path) -> Result<()> {
        let path = dir.join(CHANNELS_FILE_NAME);
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|err| TalosError::Storage(format!("create channels dir: {err}")))?;
            }
        }
        let text = serde_json::to_string_pretty(self).map_err(TalosError::Serialization)?;
        std::fs::write(&path, text)
            .map_err(|err| TalosError::Storage(format!("write channels.json: {err}")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_disables_everything() {
        let configs = ChannelConfigs::load_or_default(Path::new("/nonexistent"));
        assert!(!configs.telegram.enabled);
        assert!(!configs.discord.enabled);
    }

    #[test]
    fn corrupt_file_fails_closed() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join(CHANNELS_FILE_NAME), "{oops").expect("writes");
        let configs = ChannelConfigs::load_or_default(dir.path());
        assert!(!configs.telegram.enabled);
    }

    #[test]
    fn save_round_trip_is_secret_free() {
        let dir = tempfile::tempdir().expect("temp dir");
        let configs = ChannelConfigs {
            telegram: TelegramChannelConfig {
                enabled: true,
                allowed_chat_ids: vec!["777".to_owned()],
                poll_interval_secs: 15,
            },
            discord: DiscordChannelConfig::default(),
        };
        configs.save(dir.path()).expect("saves");
        let text = std::fs::read_to_string(dir.path().join(CHANNELS_FILE_NAME)).expect("reads");
        assert!(!text.contains("token"), "no secrets on disk");
        let back = ChannelConfigs::load_or_default(dir.path());
        assert!(back.telegram.enabled);
        assert_eq!(back.telegram.allowed_chat_ids, vec!["777".to_owned()]);
    }
}
