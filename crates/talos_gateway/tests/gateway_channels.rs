//! Channel routing proof: a mock Telegram message flows
//! adapter → allowlist → Supervisor → PolicyBroker → mock reply.
//!
//! Covers the three security outcomes: benign reads execute and reply,
//! wildcard-destructive commands are broker-denied, high-risk commands park
//! in `waiting_policy` with a HITL reply, and non-allowlisted senders are
//! dropped silently.

use async_trait::async_trait;
use channels::{ChannelConfigs, ChannelId, MockChannelAdapter, TelegramChannelConfig};
use talos_gateway::GatewayState;

fn allowlisted_registry(adapter: std::sync::Arc<MockChannelAdapter>) -> channels::ChannelRegistry {
    let configs = ChannelConfigs {
        telegram: TelegramChannelConfig {
            enabled: true,
            allowed_chat_ids: vec!["chat-9".to_owned()],
            poll_interval_secs: 30,
        },
        discord: channels::DiscordChannelConfig::default(),
    };
    channels::ChannelRegistry::from_adapters(
        vec![adapter as std::sync::Arc<dyn channels::ChannelAdapter>],
        configs,
    )
}

fn incoming(text: &str) -> channels::IncomingMessage {
    MockChannelAdapter::message("m-1", "chat-9", text)
}

#[tokio::test]
async fn telegram_message_triggers_task_and_completed_reply() {
    let adapter = channels::mock_telegram();
    adapter.inject(incoming("summarize the ledger"));
    let state = GatewayState::new_in_memory_with_channels(allowlisted_registry(adapter.clone()))
        .await
        .expect("state");
    state.poll_channels().await;
    // System-initiated task executed through the broker.
    let tasks = evidence_store::list_tasks(&state.db, 10)
        .await
        .expect("lists");
    let view = tasks
        .iter()
        .find(|task| task.title == "[telegram] summarize the ledger")
        .expect("channel task submitted");
    assert_eq!(view.status, "completed");
    // Tool receipt / execution outcome routed back through the adapter.
    let sent = adapter.take_sent();
    assert_eq!(sent.len(), 1, "exactly one reply");
    assert_eq!(sent[0].0, "chat-9");
    assert!(
        sent[0].1.contains("Task completed"),
        "completed reply: {}",
        sent[0].1
    );
}

#[tokio::test]
async fn remote_destructive_command_is_broker_denied() {
    let adapter = channels::mock_telegram();
    adapter.inject(incoming("fs.delete_all"));
    let state = GatewayState::new_in_memory_with_channels(allowlisted_registry(adapter.clone()))
        .await
        .expect("state");
    state.poll_channels().await;
    let tasks = evidence_store::list_tasks(&state.db, 10)
        .await
        .expect("lists");
    let view = tasks
        .iter()
        .find(|task| task.title == "[telegram] fs.delete_all")
        .expect("channel task submitted");
    assert_eq!(view.status, "failed", "denied tasks never execute");
    let sent = adapter.take_sent();
    assert_eq!(sent.len(), 1);
    assert!(
        sent[0].1.contains("Denied by policy broker"),
        "denial reply: {}",
        sent[0].1
    );
    assert!(
        sent[0].1.contains("default deny"),
        "broker reason surfaced: {}",
        sent[0].1
    );
}

#[tokio::test]
async fn high_risk_remote_task_parks_with_hitl_reply() {
    let adapter = channels::mock_telegram();
    adapter.inject(incoming("delete the temp files"));
    let state = GatewayState::new_in_memory_with_channels(allowlisted_registry(adapter.clone()))
        .await
        .expect("state");
    state.poll_channels().await;
    let tasks = evidence_store::list_tasks(&state.db, 10)
        .await
        .expect("lists");
    let view = tasks
        .iter()
        .find(|task| task.title == "[telegram] delete the temp files")
        .expect("channel task submitted");
    assert_eq!(view.status, "waiting_policy");
    let sent = adapter.take_sent();
    assert_eq!(sent.len(), 1);
    assert!(
        sent[0]
            .1
            .contains("Task requires desktop approval. Opening HITL gate."),
        "HITL reply: {}",
        sent[0].1
    );
}

#[tokio::test]
async fn non_allowlisted_sender_is_dropped_silently() {
    let adapter = channels::mock_telegram();
    adapter.inject(MockChannelAdapter::message(
        "m-9",
        "evil-chat",
        "summarize the ledger",
    ));
    let state = GatewayState::new_in_memory_with_channels(allowlisted_registry(adapter.clone()))
        .await
        .expect("state");
    state.poll_channels().await;
    let tasks = evidence_store::list_tasks(&state.db, 10)
        .await
        .expect("lists");
    assert!(tasks.is_empty(), "no task for non-allowlisted sender");
    assert!(adapter.take_sent().is_empty(), "no reply (no oracle)");
}

#[tokio::test]
async fn discord_message_routes_through_discord_adapter() {
    let adapter = channels::mock_telegram();
    let discord = std::sync::Arc::new(MockDiscord(adapter.clone()));
    let configs = ChannelConfigs {
        telegram: TelegramChannelConfig::default(),
        discord: channels::DiscordChannelConfig {
            enabled: true,
            allowed_channel_ids: vec!["chan-1".to_owned()],
            channel_id: Some("chan-1".to_owned()),
            poll_interval_secs: 30,
        },
    };
    let state =
        GatewayState::new_in_memory_with_channels(channels::ChannelRegistry::from_adapters(
            vec![discord as std::sync::Arc<dyn channels::ChannelAdapter>],
            configs,
        ))
        .await
        .expect("state");
    // Directly exercise the Discord allowlist + routing path.
    state
        .handle_channel_message(
            ChannelId::Discord,
            &channels::IncomingMessage {
                channel: ChannelId::Discord,
                message_id: "d-1".to_owned(),
                sender_id: "u-9".to_owned(),
                chat_id: "chan-1".to_owned(),
                text: "summarize the ledger".to_owned(),
                received_at: talos_common::now_iso(),
            },
        )
        .await;
    let sent = adapter.take_sent();
    assert_eq!(sent.len(), 1);
    assert!(sent[0].1.contains("Task completed"));
}

/// Test-only Discord-flavored mock (delegates storage to a Telegram mock so
/// the test can observe sends while the channel id is Discord).
#[derive(Debug)]
struct MockDiscord(std::sync::Arc<MockChannelAdapter>);

#[async_trait]
impl channels::ChannelAdapter for MockDiscord {
    fn channel_id(&self) -> ChannelId {
        ChannelId::Discord
    }

    async fn send_message(
        &self,
        to: &str,
        text: &str,
    ) -> Result<channels::MessageReceipt, channels::ChannelError> {
        self.0.send_message(to, text).await
    }

    async fn receive_updates(
        &self,
    ) -> Result<Vec<channels::IncomingMessage>, channels::ChannelError> {
        Ok(Vec::new())
    }

    fn receive_webhook(
        &self,
        payload: &serde_json::Value,
    ) -> Result<Vec<channels::IncomingMessage>, channels::ChannelError> {
        self.0.receive_webhook(payload)
    }
}
