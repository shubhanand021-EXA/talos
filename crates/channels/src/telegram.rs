//! Telegram adapter: Bot API long-polling (`getUpdates`) + `sendMessage`.
//!
//! Token source: OS keychain (`channel.telegram.token`) resolved by the
//! registry; tests inject inline. Offset tracking is in-memory (a restart
//! replays at most one poll window; `parse_task` + task-id dedup make replays
//! harmless — the gateway skips already-seen platform message ids).

use async_trait::async_trait;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use llm_gateway::{HttpClient, MockHttpClient};

use crate::adapter::ChannelAdapter;
use crate::model::{ChannelError, ChannelId, IncomingMessage, MessageReceipt};

/// Telegram Bot API surface used by the adapter.
#[derive(Debug)]
pub struct TelegramAdapter {
    token: String,
    http: Arc<dyn HttpClient>,
    next_offset: AtomicU64,
}

impl TelegramAdapter {
    /// Build with an inline token (tests, or registry after keychain lookup).
    #[must_use]
    pub fn new(token: impl Into<String>, http: Arc<dyn HttpClient>) -> Self {
        Self {
            token: token.into(),
            http,
            next_offset: AtomicU64::new(0),
        }
    }

    /// Build with the hermetic mock HTTP client (unit tests).
    #[must_use]
    pub fn with_mock(token: impl Into<String>) -> (Self, Arc<MockHttpClient>) {
        let http = Arc::new(MockHttpClient::default());
        (Self::new(token, http.clone()), http)
    }

    fn api(&self, method: &str) -> String {
        format!("https://api.telegram.org/bot{}/{}", self.token, method)
    }

    /// Current update offset (test hook).
    #[must_use]
    pub fn offset(&self) -> u64 {
        self.next_offset.load(Ordering::SeqCst)
    }
}

/// One parsed Telegram update (message subset only; other update kinds skip).
fn parse_update(value: &serde_json::Value) -> Option<(u64, IncomingMessage)> {
    let update_id = value.get("update_id")?.as_u64()?;
    let message = value.get("message")?;
    let message_id = message.get("message_id")?.as_u64()?.to_string();
    let chat_id = message.get("chat")?.get("id")?.to_string();
    let sender_id = message
        .get("from")
        .and_then(|from| from.get("id"))
        .map(|id| id.to_string())
        .unwrap_or_else(|| "unknown".to_owned());
    let text = message.get("text")?.as_str()?;
    let date = message.get("date")?.as_i64().unwrap_or(0);
    Some((
        update_id,
        IncomingMessage {
            channel: ChannelId::Telegram,
            message_id,
            sender_id,
            chat_id,
            text: text.to_owned(),
            received_at: chrono::DateTime::from_timestamp(date, 0)
                .map(|instant| instant.to_rfc3339())
                .unwrap_or_else(talos_common::now_iso),
        },
    ))
}

#[async_trait]
impl ChannelAdapter for TelegramAdapter {
    fn channel_id(&self) -> ChannelId {
        ChannelId::Telegram
    }

    async fn send_message(&self, to: &str, text: &str) -> Result<MessageReceipt, ChannelError> {
        let (status, body) = self
            .http
            .post_json(
                &self.api("sendMessage"),
                vec![],
                serde_json::json!({
                    "chat_id": to,
                    "text": text,
                    "parse_mode": "Markdown",
                    "disable_web_page_preview": true,
                }),
            )
            .await
            .map_err(ChannelError::Transport)?;
        if !(200..300).contains(&status) {
            return Err(ChannelError::Rejected(format!(
                "sendMessage HTTP {status}: {}",
                String::from_utf8_lossy(&body)
                    .chars()
                    .take(256)
                    .collect::<String>()
            )));
        }
        let platform_id = serde_json::from_slice::<serde_json::Value>(&body)
            .ok()
            .and_then(|json| json.pointer("/result/message_id").cloned())
            .map(|id| id.to_string());
        Ok(MessageReceipt {
            channel: ChannelId::Telegram,
            platform_id,
        })
    }

    async fn receive_updates(&self) -> Result<Vec<IncomingMessage>, ChannelError> {
        let offset = self.offset();
        let (status, body) = self
            .http
            .post_json(
                &self.api("getUpdates"),
                vec![],
                serde_json::json!({ "offset": offset, "timeout": 30, "allowed_updates": ["message"] }),
            )
            .await
            .map_err(ChannelError::Transport)?;
        if !(200..300).contains(&status) {
            return Err(ChannelError::Rejected(format!("getUpdates HTTP {status}")));
        }
        let json: serde_json::Value = serde_json::from_slice(&body)
            .map_err(|err| ChannelError::Malformed(err.to_string()))?;
        let updates = json
            .get("result")
            .and_then(|result| result.as_array())
            .ok_or_else(|| ChannelError::Malformed("getUpdates missing result[]".to_owned()))?;
        let mut messages = Vec::new();
        let mut max_id = None::<u64>;
        for update in updates {
            if let Some((update_id, message)) = parse_update(update) {
                max_id = Some(max_id.map_or(update_id, |max: u64| max.max(update_id)));
                messages.push(message);
            }
        }
        if let Some(max_id) = max_id {
            self.next_offset.store(max_id + 1, Ordering::SeqCst);
        }
        Ok(messages)
    }

    fn receive_webhook(
        &self,
        payload: &serde_json::Value,
    ) -> Result<Vec<IncomingMessage>, ChannelError> {
        // A pushed Telegram `Update` is one object (getUpdates wraps in result[]).
        match parse_update(payload) {
            Some((_, message)) => Ok(vec![message]),
            None => Err(ChannelError::Malformed("not a message Update".to_owned())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn update(id: u64, text: &str) -> serde_json::Value {
        serde_json::json!({
            "update_id": id,
            "message": {
                "message_id": id * 10,
                "from": { "id": 4242 },
                "chat": { "id": 777 },
                "date": 1_792_000_000,
                "text": text,
            }
        })
    }

    #[tokio::test]
    async fn receive_updates_parses_and_advances_offset() {
        let (adapter, http) = TelegramAdapter::with_mock("TOKEN");
        http.push_response(
            200,
            serde_json::json!({ "ok": true, "result": [update(11, "summarize the ledger"), update(12, "hi")] }),
        );
        let messages = adapter.receive_updates().await.expect("polls");
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].chat_id, "777");
        assert_eq!(messages[0].text, "summarize the ledger");
        assert_eq!(adapter.offset(), 13);
        // Empty windows keep the cursor and return nothing.
        http.push_response(200, serde_json::json!({ "ok": true, "result": [] }));
        assert!(adapter.receive_updates().await.expect("polls").is_empty());
        assert_eq!(adapter.offset(), 13);
    }

    #[tokio::test]
    async fn send_message_posts_markdown_body() {
        let (adapter, http) = TelegramAdapter::with_mock("TOKEN");
        http.push_response(
            200,
            serde_json::json!({ "ok": true, "result": { "message_id": 99 } }),
        );
        let receipt = adapter.send_message("777", "✅ done").await.expect("sends");
        assert_eq!(receipt.platform_id.as_deref(), Some("99"));
        assert_eq!(http.call_count(), 1);
    }

    #[tokio::test]
    async fn transport_errors_surface() {
        let (adapter, http) = TelegramAdapter::with_mock("TOKEN");
        *http.error.lock().expect("mock") = Some("boom".to_owned());
        assert!(adapter.receive_updates().await.is_err());
        assert!(adapter.send_message("1", "x").await.is_err());
    }

    #[test]
    fn receive_webhook_parses_single_update() {
        let (adapter, _) = TelegramAdapter::with_mock("TOKEN");
        let messages = adapter
            .receive_webhook(&update(7, "status"))
            .expect("parses");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].message_id, "70");
        assert!(adapter
            .receive_webhook(&serde_json::json!({"edited_message": {}}))
            .is_err());
    }
}
