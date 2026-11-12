//! Hermetic adapter for tests: queued inbound, recorded outbound.

use async_trait::async_trait;
use std::sync::{Arc, Mutex};

use crate::adapter::ChannelAdapter;
use crate::model::{ChannelError, ChannelId, IncomingMessage, MessageReceipt};

/// In-memory channel double. Tests inject inbound messages and assert on
/// recorded outbound text — no network, no tokens.
#[derive(Debug, Default)]
pub struct MockChannelAdapter {
    channel: MockChannelId,
    inbound: Mutex<Vec<IncomingMessage>>,
    outbound: Mutex<Vec<(String, String)>>,
}

#[derive(Debug, Default, Clone, Copy)]
enum MockChannelId {
    #[default]
    Telegram,
}

/// Build a Telegram-flavored mock.
#[must_use]
pub fn mock_telegram() -> Arc<MockChannelAdapter> {
    Arc::new(MockChannelAdapter::default())
}

impl MockChannelAdapter {
    /// Queue one inbound message.
    pub fn inject(&self, message: IncomingMessage) {
        self.inbound.lock().expect("mock").push(message);
    }

    /// Drain recorded outbound `(to, text)` pairs.
    pub fn take_sent(&self) -> Vec<(String, String)> {
        std::mem::take(&mut *self.outbound.lock().expect("mock"))
    }

    /// Build a minimal inbound text message.
    #[must_use]
    pub fn message(message_id: &str, chat_id: &str, text: &str) -> IncomingMessage {
        IncomingMessage {
            channel: ChannelId::Telegram,
            message_id: message_id.to_owned(),
            sender_id: "user-1".to_owned(),
            chat_id: chat_id.to_owned(),
            text: text.to_owned(),
            received_at: talos_common::now_iso(),
        }
    }
}

#[async_trait]
impl ChannelAdapter for MockChannelAdapter {
    fn channel_id(&self) -> ChannelId {
        match self.channel {
            MockChannelId::Telegram => ChannelId::Telegram,
        }
    }

    async fn send_message(&self, to: &str, text: &str) -> Result<MessageReceipt, ChannelError> {
        self.outbound
            .lock()
            .expect("mock")
            .push((to.to_owned(), text.to_owned()));
        Ok(MessageReceipt {
            channel: self.channel_id(),
            platform_id: None,
        })
    }

    async fn receive_updates(&self) -> Result<Vec<IncomingMessage>, ChannelError> {
        Ok(std::mem::take(&mut *self.inbound.lock().expect("mock")))
    }

    fn receive_webhook(
        &self,
        payload: &serde_json::Value,
    ) -> Result<Vec<IncomingMessage>, ChannelError> {
        let text = payload
            .get("text")
            .and_then(|text| text.as_str())
            .ok_or_else(|| ChannelError::Malformed("mock webhook needs {text}".to_owned()))?;
        Ok(vec![Self::message("webhook-1", "chat-1", text)])
    }
}
