//! Channel registry: adapter ownership, keychain-backed credentials,
//! allowlists, and the UI message log.
//!
//! The registry moves bytes only. Authorization (allowlists here are a
//! first-line spam filter, not authority) and execution live in the gateway:
//! every message still passes the PolicyBroker before anything runs.

use std::collections::HashSet;
use std::sync::Arc;

use llm_gateway::HttpClient;
use talos_common::{Result, TalosError};

use crate::adapter::ChannelAdapter;
use crate::config::{ChannelConfigs, DISCORD_TOKEN_KEY, DISCORD_WEBHOOK_KEY, TELEGRAM_TOKEN_KEY};
use crate::discord::DiscordAdapter;
use crate::model::{ChannelId, IncomingMessage};
use crate::telegram::TelegramAdapter;

/// Keychain service for channel secrets.
pub const CHANNEL_SECRET_SERVICE: &str = "com.talos.desktop";

/// One logged message row for the UI Channels tab (metadata only).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoggedMessage {
    /// Channel the message crossed.
    pub channel: ChannelId,
    /// Direction (`in` | `out`).
    pub direction: String,
    /// Peer conversation (chat / channel id).
    pub chat_id: String,
    /// Text excerpt (first 256 chars, never secrets).
    pub excerpt: String,
    /// RFC 3339 time.
    pub at: String,
}

/// Read model for the UI Channels tab (never carries tokens).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelStatusView {
    /// Channel id.
    pub channel: ChannelId,
    /// Whether the adapter polls.
    pub enabled: bool,
    /// Whether a credential is present in the keychain.
    pub linked: bool,
    /// Last poll outcome, if any.
    pub last_poll_at: Option<String>,
    /// Messages seen this process.
    pub message_count: u64,
}

/// Owned adapters + secret-free config + bounded message log.
pub struct ChannelRegistry {
    adapters: Vec<Arc<dyn ChannelAdapter>>,
    configs: ChannelConfigs,
    seen_ids: HashSet<String>,
    log: Vec<LoggedMessage>,
    message_count: u64,
}

impl std::fmt::Debug for ChannelRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChannelRegistry")
            .field("adapters", &self.adapters.len())
            .field("message_count", &self.message_count)
            .finish()
    }
}

impl Default for ChannelRegistry {
    fn default() -> Self {
        Self::empty(ChannelConfigs::default())
    }
}

impl ChannelRegistry {
    /// Empty registry (disabled set; tests inject mocks via
    /// [`ChannelRegistry::from_adapters`]).
    #[must_use]
    pub fn empty(configs: ChannelConfigs) -> Self {
        Self {
            adapters: Vec::new(),
            configs,
            seen_ids: HashSet::new(),
            log: Vec::new(),
            message_count: 0,
        }
    }

    /// Build live adapters for every enabled channel with a keychain
    /// credential. Missing credentials skip the channel with a log line
    /// (fail closed, never crash the daemon).
    #[must_use]
    pub fn from_config(
        configs: &ChannelConfigs,
        vault: &talos_secrets::SecretVault,
        http: Arc<dyn HttpClient>,
    ) -> Self {
        let mut registry = Self::empty(configs.clone());
        if configs.telegram.enabled {
            match vault.get_secret(CHANNEL_SECRET_SERVICE, TELEGRAM_TOKEN_KEY) {
                Ok(Some(token)) if !token.trim().is_empty() => {
                    registry
                        .adapters
                        .push(Arc::new(TelegramAdapter::new(token, http.clone())));
                }
                Ok(_) => {
                    eprintln!("[talos-channels] telegram enabled but unlinked; skipping");
                }
                Err(err) => {
                    eprintln!("[talos-channels] telegram keychain lookup failed: {err}");
                }
            }
        }
        if configs.discord.enabled {
            let token = vault
                .get_secret(CHANNEL_SECRET_SERVICE, DISCORD_TOKEN_KEY)
                .ok()
                .flatten();
            let webhook = vault
                .get_secret(CHANNEL_SECRET_SERVICE, DISCORD_WEBHOOK_KEY)
                .ok()
                .flatten();
            if token
                .as_deref()
                .is_some_and(|token| !token.trim().is_empty())
                || webhook
                    .as_deref()
                    .is_some_and(|hook| !hook.trim().is_empty())
            {
                registry.adapters.push(Arc::new(DiscordAdapter::new(
                    token,
                    webhook,
                    configs.discord.channel_id.clone(),
                    http,
                )));
            } else {
                eprintln!("[talos-channels] discord enabled but unlinked; skipping");
            }
        }
        registry
    }

    /// Build around prebuilt adapters (tests).
    #[must_use]
    pub fn from_adapters(adapters: Vec<Arc<dyn ChannelAdapter>>, configs: ChannelConfigs) -> Self {
        let mut registry = Self::empty(configs);
        registry.adapters = adapters;
        registry
    }

    /// Adapter count (test hook).
    #[must_use]
    pub fn len(&self) -> usize {
        self.adapters.len()
    }

    /// True when no adapters are owned.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.adapters.is_empty()
    }

    /// Status per configured channel for the UI (never carries tokens).
    #[must_use]
    pub fn status(&self, vault: &talos_secrets::SecretVault) -> Vec<ChannelStatusView> {
        let telegram_linked = vault
            .get_secret(CHANNEL_SECRET_SERVICE, TELEGRAM_TOKEN_KEY)
            .ok()
            .flatten()
            .is_some_and(|token| !token.trim().is_empty());
        let discord_linked = vault
            .get_secret(CHANNEL_SECRET_SERVICE, DISCORD_TOKEN_KEY)
            .ok()
            .flatten()
            .is_some_and(|token| !token.trim().is_empty())
            || vault
                .get_secret(CHANNEL_SECRET_SERVICE, DISCORD_WEBHOOK_KEY)
                .ok()
                .flatten()
                .is_some_and(|hook| !hook.trim().is_empty());
        vec![
            ChannelStatusView {
                channel: ChannelId::Telegram,
                enabled: self.configs.telegram.enabled,
                linked: telegram_linked,
                last_poll_at: None,
                message_count: self.message_count,
            },
            ChannelStatusView {
                channel: ChannelId::Discord,
                enabled: self.configs.discord.enabled,
                linked: discord_linked,
                last_poll_at: None,
                message_count: self.message_count,
            },
        ]
    }

    /// Recent message log for the UI, newest first (metadata + excerpts).
    #[must_use]
    pub fn recent_messages(&self, limit: usize) -> Vec<LoggedMessage> {
        self.log.iter().rev().take(limit).cloned().collect()
    }

    /// Poll every adapter once; returns deduplicated inbound messages.
    /// Failures are logged per adapter and never fail the tick.
    pub async fn poll_once(&mut self) -> Vec<(ChannelId, IncomingMessage)> {
        let mut inbound = Vec::new();
        for index in 0..self.adapters.len() {
            let adapter = self.adapters[index].clone();
            match adapter.receive_updates().await {
                Ok(messages) => {
                    for message in messages {
                        let key =
                            format!("{}:{}", adapter.channel_id().as_str(), message.message_id);
                        if !self.seen_ids.insert(key) {
                            continue;
                        }
                        self.message_count += 1;
                        self.push_log(&message, "in", &message.text);
                        inbound.push((adapter.channel_id(), message));
                    }
                }
                Err(err) => {
                    eprintln!(
                        "[talos-channels] poll {} failed: {err}",
                        adapter.channel_id().as_str()
                    );
                }
            }
        }
        inbound
    }

    /// Send one reply through the named adapter; records the outbound log.
    /// Unknown adapter ⇒ error (fail closed, never misroute a reply).
    pub async fn send_reply(
        &mut self,
        channel: ChannelId,
        to: &str,
        text: &str,
    ) -> Result<crate::model::MessageReceipt> {
        let adapter = self
            .adapters
            .iter()
            .find(|adapter| adapter.channel_id() == channel)
            .ok_or_else(|| TalosError::Storage(format!("no adapter for {}", channel.as_str())))?;
        let receipt = adapter.send_message(to, text).await?;
        self.push_log_raw(channel, "out", to, text);
        Ok(receipt)
    }

    /// Store a bot token / webhook URL in the keychain and enable the channel.
    pub fn link_channel(
        &mut self,
        channel: ChannelId,
        token: &str,
        vault: &talos_secrets::SecretVault,
    ) -> Result<()> {
        if token.trim().is_empty() || token.len() > 512 {
            return Err(TalosError::Storage(
                "token must be 1..=512 chars".to_owned(),
            ));
        }
        let key = match channel {
            ChannelId::Telegram => TELEGRAM_TOKEN_KEY,
            ChannelId::Discord => {
                // Discord link accepts a bot token or a webhook URL.
                if token.trim_start().starts_with("https://") {
                    DISCORD_WEBHOOK_KEY
                } else {
                    DISCORD_TOKEN_KEY
                }
            }
        };
        vault.store_secret(CHANNEL_SECRET_SERVICE, key, token)?;
        match channel {
            ChannelId::Telegram => self.configs.telegram.enabled = true,
            ChannelId::Discord => self.configs.discord.enabled = true,
        }
        Ok(())
    }

    /// Remove credentials from the keychain and disable the channel.
    /// Missing credentials are not an error (idempotent unlink).
    pub fn unlink_channel(
        &mut self,
        channel: ChannelId,
        vault: &talos_secrets::SecretVault,
    ) -> Result<()> {
        match channel {
            ChannelId::Telegram => {
                let _ = vault.delete_secret(CHANNEL_SECRET_SERVICE, TELEGRAM_TOKEN_KEY);
                self.configs.telegram.enabled = false;
            }
            ChannelId::Discord => {
                let _ = vault.delete_secret(CHANNEL_SECRET_SERVICE, DISCORD_TOKEN_KEY);
                let _ = vault.delete_secret(CHANNEL_SECRET_SERVICE, DISCORD_WEBHOOK_KEY);
                self.configs.discord.enabled = false;
            }
        }
        self.adapters
            .retain(|adapter| adapter.channel_id() != channel);
        Ok(())
    }

    /// Attach a prebuilt adapter (link flow; tests).
    pub fn add_adapter(&mut self, adapter: Arc<dyn ChannelAdapter>) {
        if !self
            .adapters
            .iter()
            .any(|owned| owned.channel_id() == adapter.channel_id())
        {
            self.adapters.push(adapter);
        }
    }

    /// Current secret-free configs (persist after link/unlink).
    #[must_use]
    pub fn configs(&self) -> &ChannelConfigs {
        &self.configs
    }

    fn push_log(&mut self, message: &IncomingMessage, direction: &str, text: &str) {
        self.push_log_raw(message.channel, direction, &message.chat_id, text);
    }

    fn push_log_raw(&mut self, channel: ChannelId, direction: &str, chat_id: &str, text: &str) {
        let excerpt: String = text.chars().take(256).collect();
        self.log.push(LoggedMessage {
            channel,
            direction: direction.to_owned(),
            chat_id: chat_id.to_owned(),
            excerpt,
            at: talos_common::now_iso(),
        });
        if self.log.len() > 50 {
            let excess = self.log.len() - 50;
            self.log.drain(..excess);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::{mock_telegram, MockChannelAdapter};

    fn vault() -> talos_secrets::SecretVault {
        talos_secrets::SecretVault::mock()
    }

    #[test]
    fn status_reports_unlinked_by_default() {
        let registry = ChannelRegistry::default();
        let status = registry.status(&vault());
        assert!(status.iter().all(|row| !row.linked));
        assert!(status.iter().all(|row| !row.enabled));
    }

    #[test]
    fn link_stores_secret_and_enables() {
        let vault = vault();
        let mut registry = ChannelRegistry::default();
        registry
            .link_channel(ChannelId::Telegram, "BOT123", &vault)
            .expect("links");
        let status = registry.status(&vault);
        let row = status
            .iter()
            .find(|row| row.channel == ChannelId::Telegram)
            .expect("row");
        assert!(row.enabled && row.linked);
        // Link is idempotent; unlink clears both.
        registry
            .unlink_channel(ChannelId::Telegram, &vault)
            .expect("unlinks");
        let status = registry.status(&vault);
        let row = status
            .iter()
            .find(|row| row.channel == ChannelId::Telegram)
            .expect("row");
        assert!(!row.enabled && !row.linked);
    }

    #[test]
    fn link_rejects_empty_tokens() {
        let mut registry = ChannelRegistry::default();
        assert!(registry
            .link_channel(ChannelId::Telegram, "  ", &vault())
            .is_err());
    }

    #[tokio::test]
    async fn poll_dedups_and_logs() {
        let adapter = mock_telegram();
        adapter.inject(MockChannelAdapter::message("m-1", "chat-9", "hello"));
        adapter.inject(MockChannelAdapter::message("m-1", "chat-9", "hello"));
        let mut registry = ChannelRegistry::from_adapters(vec![adapter], ChannelConfigs::default());
        let first = registry.poll_once().await;
        assert_eq!(first.len(), 1, "duplicate platform id fires once");
        assert!(registry.poll_once().await.is_empty());
        assert_eq!(registry.recent_messages(10).len(), 1);
        // Replies route to the owning adapter.
        registry
            .send_reply(ChannelId::Telegram, "chat-9", "hi")
            .await
            .expect("sends");
        assert_eq!(registry.recent_messages(10).len(), 2);
    }

    #[tokio::test]
    async fn send_to_unknown_channel_fails_closed() {
        // No adapter, no route: a send to an unlinked channel is refused
        // instead of misrouted, and nothing is ledgered as sent.
        let mut registry = ChannelRegistry::default();
        let err = registry
            .send_reply(ChannelId::Discord, "chan-9", "hi")
            .await
            .expect_err("unlinked channel cannot send");
        assert!(
            err.to_string().contains("no adapter"),
            "fail-closed reason: {err}"
        );
        assert!(registry.recent_messages(10).is_empty());
    }
}
