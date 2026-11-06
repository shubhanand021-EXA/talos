//! External messaging channels for the Talos gateway daemon.
//!
//! Adapters (Telegram long-polling, Discord webhook + bot-REST polling) move
//! bytes only. Every inbound message becomes a [`TaskRequest`] that the
//! gateway submits through the Supervisor and the PolicyBroker — external
//! I/O can never bypass the Pentagon. Secrets (bot tokens, webhook URLs)
//! live in the OS keychain via [`talos_secrets`]; `.talos/channels.json`
//! holds enablement + allowlists only.

pub mod adapter;
pub mod classifier;
pub mod config;
pub mod discord;
pub mod mock;
pub mod model;
pub mod registry;
pub mod telegram;

pub use adapter::ChannelAdapter;
pub use classifier::{classify_goal, GateShape};
pub use config::{ChannelConfigs, DiscordChannelConfig, TelegramChannelConfig, CHANNELS_FILE_NAME};
pub use discord::DiscordAdapter;
pub use mock::{mock_telegram, MockChannelAdapter};
pub use model::{
    render_reply, ChannelError, ChannelId, IncomingMessage, MessageReceipt, OutgoingMessage,
    TaskRequest,
};
pub use registry::{ChannelRegistry, ChannelStatusView, LoggedMessage};
pub use telegram::TelegramAdapter;
