//! Channel message model: inbound messages, outbound replies, and the
//! task-request parse. Mirrors `@talos/contracts` `channels.ts`.

use talos_common::{Result, TalosError};

/// Supported external channels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChannelId {
    /// Telegram bot (long-polling `getUpdates`, `sendMessage`).
    Telegram,
    /// Discord (webhook POST for sends, bot-REST poll for receives).
    Discord,
}

impl ChannelId {
    /// Parse a channel name (`telegram` | `discord`).
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "telegram" => Some(Self::Telegram),
            "discord" => Some(Self::Discord),
            _ => None,
        }
    }

    /// Stable string form.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Telegram => "telegram",
            Self::Discord => "discord",
        }
    }
}

/// One inbound user message, normalized across adapters.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IncomingMessage {
    /// Channel the message arrived on.
    pub channel: ChannelId,
    /// Platform message id (dedup key).
    pub message_id: String,
    /// Sender id (Telegram user id, Discord author id).
    pub sender_id: String,
    /// Conversation id (Telegram chat id, Discord channel id).
    pub chat_id: String,
    /// Raw text (1..=4096 chars after trimming).
    pub text: String,
    /// RFC 3339 receipt time.
    pub received_at: String,
}

/// One outbound reply queued for an adapter.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OutgoingMessage {
    /// Destination conversation (chat / channel id).
    pub chat_id: String,
    /// Markdown body (bounded, never secrets).
    pub text: String,
}

/// Proof an adapter accepted a reply.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MessageReceipt {
    /// Channel that sent.
    pub channel: ChannelId,
    /// Platform message id, when the platform returns one.
    pub platform_id: Option<String>,
}

/// A parsed inbound message ready for gateway submission.
#[derive(Debug, Clone)]
pub struct TaskRequest {
    /// Short title (first line or 64-char prefix).
    pub title: String,
    /// Full goal handed to the Supervisor DAG planner.
    pub goal: String,
}

/// Channel failures (transport, auth, or malformed payloads).
#[derive(Debug, Clone, thiserror::Error)]
pub enum ChannelError {
    /// HTTP transport failure. Adapter constructors MUST scrub secrets from
    /// these strings: HTTP clients echo request URLs (which carry the
    /// Telegram token) into transport errors.
    #[error("transport: {0}")]
    Transport(String),
    /// Platform rejected the call (bad token, 4xx with body excerpt).
    #[error("rejected: {0}")]
    Rejected(String),
    /// Platform rate limit (HTTP 429). Callers must NOT tight-loop: the
    /// daemon's poll tick is the only retry, so bursts decay naturally.
    /// The body is deliberately excluded (it can echo request context).
    #[error("rate limited by platform (HTTP {status}); retry on next poll")]
    RateLimited {
        /// The 429 status code (kept for log triage, without the body).
        status: u16,
    },
    /// Payload could not be parsed.
    #[error("malformed: {0}")]
    Malformed(String),
    /// Channel not linked (no token in the keychain).
    #[error("unlinked: {0}")]
    Unlinked(String),
}

/// Minimum gap between two sends on one adapter (burst pacing; the 30s poll
/// tick is the only retry, so no retry storm is possible by construction).
pub const MIN_SEND_GAP: std::time::Duration = std::time::Duration::from_secs(1);

/// How long to wait before the next send, given the last send instant.
/// Pure function of explicit instants (deterministic under test).
#[must_use]
pub fn throttle_delay(
    last_send: Option<std::time::Instant>,
    min_gap: std::time::Duration,
    now: std::time::Instant,
) -> std::time::Duration {
    match last_send.and_then(|at| at.checked_add(min_gap)) {
        Some(ready_at) if ready_at > now => ready_at.duration_since(now),
        _ => std::time::Duration::ZERO,
    }
}

impl From<ChannelError> for TalosError {
    fn from(err: ChannelError) -> Self {
        TalosError::Internal(format!("channel: {err}"))
    }
}

impl IncomingMessage {
    /// Parse into a [`TaskRequest`]. Empty/oversize texts are rejected (fail
    /// closed); overlong goals are truncated to the 4096-char ledger bound.
    pub fn parse_task(&self) -> Result<TaskRequest> {
        let text = self.text.trim();
        if text.is_empty() {
            return Err(TalosError::Storage("empty channel message".to_owned()));
        }
        if text.len() > 4096 {
            return Err(TalosError::Storage(
                "channel message exceeds 4096 chars".to_owned(),
            ));
        }
        let first_line = text.lines().next().unwrap_or(text);
        let title: String = first_line.chars().take(64).collect();
        Ok(TaskRequest {
            title: if title.trim().is_empty() {
                "channel task".to_owned()
            } else {
                title
            },
            goal: text.to_owned(),
        })
    }
}

/// Render a task outcome as channel markdown (Telegram + Discord flavors).
/// Bodies are bounded and never carry secrets, keys, or raw payloads.
#[must_use]
pub fn render_reply(kind: &str, title: &str, detail: &str) -> String {
    let title = title.chars().take(128).collect::<String>();
    let detail = detail.chars().take(1024).collect::<String>();
    match kind {
        "completed" => format!("✅ *Task completed*\n{title}\n{detail}"),
        "waiting_policy" => {
            format!("⏳ *Task requires desktop approval. Opening HITL gate.*\n{title}\n{detail}")
        }
        "denied" => format!("⛔ *Denied by policy broker*\n{title}\n{detail}"),
        _ => format!("ℹ️ *Talos update*\n{title}\n{detail}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn incoming(text: &str) -> IncomingMessage {
        IncomingMessage {
            channel: ChannelId::Telegram,
            message_id: "m-1".to_owned(),
            sender_id: "u-1".to_owned(),
            chat_id: "c-1".to_owned(),
            text: text.to_owned(),
            received_at: "2026-11-06T00:00:00Z".to_owned(),
        }
    }

    #[test]
    fn channel_id_parses_case_insensitively() {
        assert_eq!(ChannelId::parse("Telegram"), Some(ChannelId::Telegram));
        assert_eq!(ChannelId::parse("DISCORD"), Some(ChannelId::Discord));
        assert_eq!(ChannelId::parse("sms"), None);
    }

    #[test]
    fn parse_task_titles_from_first_line() {
        let request = incoming("summarize the ledger\nsecond line")
            .parse_task()
            .expect("parses");
        assert_eq!(request.title, "summarize the ledger");
        assert!(request.goal.contains("second line"));
    }

    #[test]
    fn parse_task_rejects_empty_and_oversize() {
        assert!(incoming("   ").parse_task().is_err());
        assert!(incoming(&"x".repeat(4097)).parse_task().is_err());
    }

    #[test]
    fn render_reply_bounds_and_labels() {
        let reply = render_reply("completed", &"t".repeat(200), &"d".repeat(2000));
        assert!(reply.starts_with("✅"));
        assert!(reply.len() < 200 + 2000, "bounded");
        assert!(render_reply("waiting_policy", "t", "d").contains("HITL"));
        assert!(render_reply("denied", "t", "d").contains("⛔"));
    }

    #[test]
    fn throttle_delay_paces_bursts() {
        use std::time::{Duration, Instant};
        let gap = Duration::from_secs(1);
        let now = Instant::now();
        assert_eq!(throttle_delay(None, gap, now), Duration::ZERO);
        assert_eq!(
            throttle_delay(Some(now), gap, now),
            gap,
            "back-to-back sends wait the full gap"
        );
        assert_eq!(
            throttle_delay(Some(now - Duration::from_secs(5)), gap, now),
            Duration::ZERO,
            "idle adapters send immediately"
        );
    }
}
