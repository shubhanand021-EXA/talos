//! Discord adapter: webhook POST for sends, bot-REST poll for receives.
//!
//! Sends prefer the incoming-webhook URL (no privileged intents); when no
//! webhook is configured the adapter falls back to the bot-token
//! `POST /channels/{id}/messages`. Receives poll
//! `GET /channels/{id}/messages?after=` with the bot token — no Gateway
//! websocket, no privileged message-content intent required for the daemon's
//! own command channel. The adapter skips its own outbound messages and
//! anything without text.

use async_trait::async_trait;
use std::sync::Arc;

use llm_gateway::{HttpClient, MockHttpClient};

use crate::adapter::ChannelAdapter;
use crate::model::{
    throttle_delay, ChannelError, ChannelId, IncomingMessage, MessageReceipt, MIN_SEND_GAP,
};

/// Discord REST base.
const DISCORD_API: &str = "https://discord.com/api/v10";

/// Discord adapter. Either credential suffices for its direction:
/// webhook URL for sends, bot token (+ channel id) for both directions.
#[derive(Debug)]
pub struct DiscordAdapter {
    bot_token: Option<String>,
    webhook_url: Option<String>,
    poll_channel_id: Option<String>,
    http: Arc<dyn HttpClient>,
    last_message_id: std::sync::Mutex<Option<String>>,
    last_send: std::sync::Mutex<Option<std::time::Instant>>,
}

impl DiscordAdapter {
    /// Build with explicit credentials (registry resolves them from the
    /// keychain; tests inject inline).
    #[must_use]
    pub fn new(
        bot_token: Option<String>,
        webhook_url: Option<String>,
        poll_channel_id: Option<String>,
        http: Arc<dyn HttpClient>,
    ) -> Self {
        Self {
            bot_token,
            webhook_url,
            poll_channel_id,
            http,
            last_message_id: std::sync::Mutex::new(None),
            last_send: std::sync::Mutex::new(None),
        }
    }

    /// Build with the hermetic mock HTTP client (unit tests).
    #[must_use]
    pub fn with_mock(
        bot_token: Option<String>,
        webhook_url: Option<String>,
        poll_channel_id: Option<String>,
    ) -> (Self, Arc<MockHttpClient>) {
        let http = Arc::new(MockHttpClient::default());
        (
            Self::new(bot_token, webhook_url, poll_channel_id, http.clone()),
            http,
        )
    }

    fn auth_header(&self) -> Result<Vec<(String, String)>, ChannelError> {
        self.bot_token
            .clone()
            .map(|token| vec![("Authorization".to_owned(), format!("Bot {token}"))])
            .ok_or_else(|| ChannelError::Unlinked("discord bot token missing".to_owned()))
    }

    /// Strip every credential (bot token, webhook URL) from strings that may
    /// reach logs, errors, or evidence. Webhook URLs embed their secret, and
    /// HTTP clients echo request URLs into transport errors.
    fn scrub(&self, text: &str) -> String {
        let mut out = text.to_owned();
        for secret in self
            .bot_token
            .iter()
            .chain(self.webhook_url.iter())
            .filter(|secret| !secret.is_empty())
        {
            out = out.replace(secret.as_str(), "[redacted]");
        }
        out
    }

    /// Map a non-2xx status to a typed error. 429 is explicit (no body:
    /// callers must not tight-loop; the poll tick is the only retry).
    fn reject(context: &str, status: u16, body: &[u8]) -> ChannelError {
        if status == 429 {
            return ChannelError::RateLimited { status };
        }
        ChannelError::Rejected(format!(
            "{context} HTTP {status}: {}",
            String::from_utf8_lossy(body)
                .chars()
                .take(256)
                .collect::<String>()
        ))
    }

    /// Pace outbound sends (burst guard; see `MIN_SEND_GAP`).
    async fn pace_send(&self) {
        let delay = {
            let last = *self.last_send.lock().expect("send clock");
            throttle_delay(last, MIN_SEND_GAP, std::time::Instant::now())
        };
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        *self.last_send.lock().expect("send clock") = Some(std::time::Instant::now());
    }
}

/// Parse one Discord message object. Bot-authored messages (including our
/// own replies) never become tasks: the command channel is human-driven,
/// and re-ingesting our own output would self-trigger an unbounded loop.
fn parse_message(value: &serde_json::Value) -> Option<IncomingMessage> {
    let message_id = value.get("id")?.as_str()?.to_owned();
    let author = value.get("author")?;
    if author.get("bot")?.as_bool().unwrap_or(false) {
        return None;
    }
    let text = value.get("content")?.as_str()?;
    if text.trim().is_empty() {
        return None;
    }
    Some(IncomingMessage {
        channel: ChannelId::Discord,
        message_id,
        sender_id: author.get("id")?.as_str()?.to_owned(),
        chat_id: value.get("channel_id")?.as_str()?.to_owned(),
        text: text.to_owned(),
        received_at: value
            .get("timestamp")
            .and_then(|timestamp| timestamp.as_str())
            .unwrap_or("1970-01-01T00:00:00Z")
            .to_owned(),
    })
}

#[async_trait]
impl ChannelAdapter for DiscordAdapter {
    fn channel_id(&self) -> ChannelId {
        ChannelId::Discord
    }

    async fn send_message(&self, to: &str, text: &str) -> Result<MessageReceipt, ChannelError> {
        self.pace_send().await;
        let body = serde_json::json!({ "content": text });
        if let Some(webhook) = self.webhook_url.clone() {
            let url = format!("{webhook}?wait=true");
            let (status, bytes) = self
                .http
                .post_json(&url, vec![], body)
                .await
                .map_err(|err| ChannelError::Transport(self.scrub(&err)))?;
            if !(200..300).contains(&status) {
                return Err(Self::reject("webhook", status, &bytes));
            }
            let platform_id = serde_json::from_slice::<serde_json::Value>(&bytes)
                .ok()
                .and_then(|json| json.get("id").cloned())
                .and_then(|id| id.as_str().map(str::to_owned));
            return Ok(MessageReceipt {
                channel: ChannelId::Discord,
                platform_id,
            });
        }
        let channel = self
            .poll_channel_id
            .clone()
            .or_else(|| Some(to.to_owned()))
            .ok_or_else(|| ChannelError::Unlinked("discord channel id missing".to_owned()))?;
        let headers = self.auth_header()?;
        let (status, bytes) = self
            .http
            .post_json(
                &format!("{DISCORD_API}/channels/{channel}/messages"),
                headers,
                body,
            )
            .await
            .map_err(|err| ChannelError::Transport(self.scrub(&err)))?;
        if !(200..300).contains(&status) {
            return Err(Self::reject("bot send", status, &bytes));
        }
        let platform_id = serde_json::from_slice::<serde_json::Value>(&bytes)
            .ok()
            .and_then(|json| json.get("id").cloned())
            .and_then(|id| id.as_str().map(str::to_owned));
        Ok(MessageReceipt {
            channel: ChannelId::Discord,
            platform_id,
        })
    }

    async fn receive_updates(&self) -> Result<Vec<IncomingMessage>, ChannelError> {
        let channel = self
            .poll_channel_id
            .clone()
            .ok_or_else(|| ChannelError::Unlinked("discord channel id missing".to_owned()))?;
        let headers = self.auth_header()?;
        let mut url = format!("{DISCORD_API}/channels/{channel}/messages?limit=25");
        if let Some(after) = self.last_message_id.lock().expect("cursor").clone() {
            url = format!("{url}&after={after}");
        }
        let (status, bytes) = self
            .http
            .get(&url, headers)
            .await
            .map_err(|err| ChannelError::Transport(self.scrub(&err)))?;
        if !(200..300).contains(&status) {
            return Err(Self::reject("bot poll", status, &bytes));
        }
        let json: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|err| ChannelError::Malformed(err.to_string()))?;
        let messages = json.as_array().ok_or_else(|| {
            ChannelError::Malformed("discord messages must be an array".to_owned())
        })?;
        // Filter against the pre-batch cursor (wire order is newest-first;
        // comparing within the batch would drop older siblings).
        let stored = self.last_message_id.lock().expect("cursor").clone();
        let mut incoming = Vec::new();
        let mut max_id = stored.clone();
        for message in messages {
            let Some(parsed) = parse_message(message) else {
                continue;
            };
            if stored
                .as_deref()
                .is_some_and(|cursor| parsed.message_id.as_str() <= cursor)
            {
                continue;
            }
            max_id = Some(match max_id {
                Some(current) => current.max(parsed.message_id.clone()),
                None => parsed.message_id.clone(),
            });
            incoming.push(parsed);
        }
        // Discord returns newest-first; present oldest-first for ordering.
        incoming.reverse();
        *self.last_message_id.lock().expect("cursor") = max_id;
        Ok(incoming)
    }

    fn receive_webhook(
        &self,
        payload: &serde_json::Value,
    ) -> Result<Vec<IncomingMessage>, ChannelError> {
        // A pushed Discord MESSAGE_CREATE dispatch carries the message inline
        // (`{ "t": "MESSAGE_CREATE", "d": { ...message... } }`).
        let message = payload.get("d").unwrap_or(payload);
        match parse_message(message) {
            Some(parsed) => Ok(vec![parsed]),
            None => Err(ChannelError::Malformed(
                "not a text message dispatch".to_owned(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn discord_message(id: &str, content: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "channel_id": "chan-9",
            "author": { "id": "user-3", "bot": false },
            "content": content,
            "timestamp": "2026-11-06T10:00:00.000Z",
        })
    }

    #[tokio::test]
    async fn webhook_send_posts_content() {
        let (adapter, http) = DiscordAdapter::with_mock(
            None,
            Some("https://discord.com/api/webhooks/abc".to_owned()),
            None,
        );
        http.push_response(200, serde_json::json!({ "id": "msg-1" }));
        let receipt = adapter
            .send_message("chan-9", "✅ done")
            .await
            .expect("sends");
        assert_eq!(receipt.platform_id.as_deref(), Some("msg-1"));
        assert_eq!(http.call_count(), 1);
    }

    #[tokio::test]
    async fn bot_send_falls_back_without_webhook() {
        let (adapter, http) =
            DiscordAdapter::with_mock(Some("BOT".to_owned()), None, Some("chan-9".to_owned()));
        http.push_response(200, serde_json::json!({ "id": "msg-2" }));
        let receipt = adapter.send_message("chan-9", "hi").await.expect("sends");
        assert_eq!(receipt.platform_id.as_deref(), Some("msg-2"));
    }

    #[tokio::test]
    async fn receive_poll_parses_and_cursors() {
        let (adapter, http) =
            DiscordAdapter::with_mock(Some("BOT".to_owned()), None, Some("chan-9".to_owned()));
        http.push_response(
            200,
            serde_json::json!([
                discord_message("200", "second"),
                discord_message("100", "first")
            ]),
        );
        let messages = adapter.receive_updates().await.expect("polls");
        assert_eq!(messages.len(), 2);
        // Oldest-first despite newest-first wire order.
        assert_eq!(messages[0].message_id, "100");
        assert_eq!(messages[1].text, "second");
        // Second poll only returns newer messages.
        http.push_response(200, serde_json::json!([discord_message("200", "second")]));
        assert!(adapter.receive_updates().await.expect("polls").is_empty());
    }

    #[test]
    fn receive_webhook_parses_dispatch() {
        let (adapter, _) = DiscordAdapter::with_mock(None, None, None);
        let payload = serde_json::json!({ "t": "MESSAGE_CREATE", "d": discord_message("5", "go") });
        let messages = adapter.receive_webhook(&payload).expect("parses");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].sender_id, "user-3");
    }

    #[test]
    fn bot_authored_messages_never_become_tasks() {
        // Our own replies (and any other bot) must not self-trigger: the
        // adapter drops every bot-authored message before the gateway sees it.
        let mut own = discord_message("6", "✅ done");
        own["author"]["bot"] = serde_json::json!(true);
        own["author"]["id"] = serde_json::json!("own-bot-id");
        assert!(
            super::parse_message(&own).is_none(),
            "bot-authored message skipped"
        );
        assert!(super::parse_message(&discord_message("7", "hi")).is_some());
    }

    #[tokio::test]
    async fn transport_errors_never_carry_secrets() {
        let (adapter, http) = DiscordAdapter::with_mock(
            Some("DISCORD-TOKEN-ABC".to_owned()),
            Some("https://discord.com/api/webhooks/hook/SECRET".to_owned()),
            Some("chan-9".to_owned()),
        );
        // reqwest echoes request URLs into transport errors; the webhook URL
        // carries its secret — the adapter must scrub before propagating.
        *http.error.lock().expect("mock") = Some(
            "error sending request for url (https://discord.com/api/webhooks/hook/SECRET?wait=true): boom".to_owned(),
        );
        let err = adapter
            .send_message("chan-9", "x")
            .await
            .expect_err("transport fails");
        assert!(
            !err.to_string().contains("SECRET"),
            "webhook secret scrubbed from: {err}"
        );
    }

    #[tokio::test]
    async fn rate_limit_maps_to_typed_error_without_body() {
        let (adapter, http) = DiscordAdapter::with_mock(
            None,
            Some("https://discord.com/api/webhooks/abc".to_owned()),
            None,
        );
        http.push_response(429, serde_json::json!({ "retry_after": 1.2 }));
        let err = adapter
            .send_message("chan-9", "x")
            .await
            .expect_err("429 surfaces");
        assert!(
            matches!(err, ChannelError::RateLimited { status: 429 }),
            "typed 429, got: {err:?}"
        );
    }
}
