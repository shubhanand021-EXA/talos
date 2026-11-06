//! `ChannelAdapter` trait: the byte-moving boundary.
//!
//! Adapters never interpret, authorize, or execute. They translate platform
//! payloads into [`IncomingMessage`]s and deliver [`OutgoingMessage`] text.
//! Policy evaluation and task execution happen exclusively in the gateway.

use async_trait::async_trait;

use crate::model::{ChannelError, ChannelId, IncomingMessage, MessageReceipt};

/// One external messaging surface (Telegram, Discord, ...).
///
/// Implementations must be cheap to share (`Arc<dyn ChannelAdapter>`) and
/// must never log or persist message text, tokens, or webhook URLs.
#[async_trait]
pub trait ChannelAdapter: Send + Sync + std::fmt::Debug {
    /// Stable channel id (`telegram` | `discord`).
    fn channel_id(&self) -> ChannelId;

    /// Deliver `text` to conversation `to` (chat id / channel id).
    async fn send_message(&self, to: &str, text: &str) -> Result<MessageReceipt, ChannelError>;

    /// Poll for new inbound messages (Telegram `getUpdates` long-poll,
    /// Discord bot-REST `GET /channels/{id}/messages`). Returns only
    /// messages not seen before; adapters track their own cursor.
    async fn receive_updates(&self) -> Result<Vec<IncomingMessage>, ChannelError>;

    /// Parse one pushed webhook body into inbound messages (Telegram
    /// `Update`, Discord gateway dispatch). The loopback daemon has no
    /// public ingress today — this is the seam a future webhook endpoint
    /// calls — but parsing is live and unit-tested now.
    fn receive_webhook(
        &self,
        payload: &serde_json::Value,
    ) -> Result<Vec<IncomingMessage>, ChannelError>;
}
