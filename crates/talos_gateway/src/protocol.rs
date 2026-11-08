//! Gateway wire protocol: Rust mirror of `@talos/contracts` `gateway.ts`.
//!
//! Every frame carries `protocol: "talos-gateway/1"`. Commands flow
//! client → daemon; events flow daemon → every connected client. No layer may
//! redefine these shapes: the TS Zod schemas are the other half of this file.

use serde::{Deserialize, Serialize};

/// Protocol tag every frame must carry.
pub const GATEWAY_PROTOCOL_VERSION: &str = "talos-gateway/1";
/// Loopback bind port.
pub const GATEWAY_PORT: u16 = 8080;
/// WebSocket route served by the daemon.
pub const GATEWAY_ROUTE: &str = "/talos";
/// Heartbeat cadence (ms). Clients missing 3 pongs are `DISCONNECTED`.
pub const GATEWAY_HEARTBEAT_INTERVAL_MS: u64 = 5_000;

/// Commands a client may send (mirrors `GatewayCommandKind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GatewayCommandKind {
    /// Enqueue and execute a durable task through the policy broker.
    SubmitTask,
    /// Request cancellation for one durable task.
    CancelTask,
    /// Human grant/deny of a pending policy approval.
    ApprovePolicy,
    /// Heartbeat probe (answered with `Pong`).
    Ping,
    /// Status snapshot probe (answered with a `Pong` carrying status).
    GetStatus,
    /// Route any of the 96 Tauri IPC commands through the daemon.
    ForwardIpc,
}

/// Payload for `SubmitTask`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewaySubmitTask {
    /// Human-readable title (1..=256 chars).
    pub title: String,
    /// Optional detail.
    pub description: Option<String>,
    /// `low` | `normal` | `high` | `critical` (defaults to `normal`).
    #[serde(default = "default_priority")]
    pub priority: String,
    /// Deterministic mock scenario (defaults to `quick_success`).
    #[serde(default = "default_scenario")]
    pub scenario: String,
}

fn default_priority() -> String {
    "normal".to_owned()
}

fn default_scenario() -> String {
    "quick_success".to_owned()
}

/// Payload for `CancelTask`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayCancelTask {
    /// Task to cancel (1..=128 chars).
    pub task_id: String,
}

/// Payload for `ApprovePolicy`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayApprovePolicy {
    /// Pending approval id (1..=128 chars).
    pub approval_id: String,
    /// True grants, false denies.
    pub approved: bool,
    /// Human rationale for the verdict.
    pub reason: Option<String>,
}

/// Payload for `ForwardIpc`: routes any of the 96 Tauri commands through the
/// daemon over the same socket.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayForwardIpc {
    /// Tauri IPC command name (e.g. `submit_task`).
    pub command: String,
    /// Command args (camelCase keys, mirroring the Tauri surface).
    #[serde(default)]
    pub args: std::collections::HashMap<String, serde_json::Value>,
}

/// One command frame: `{ protocol, id, command, payload }`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayCommand {
    /// Protocol tag (always `talos-gateway/1`).
    pub protocol: String,
    /// Client-chosen correlation id (1..=128 chars).
    pub id: String,
    /// Command kind.
    pub command: GatewayCommandKind,
    /// Kind-specific payload.
    pub payload: serde_json::Value,
}

impl GatewayCommand {
    /// Build a versioned command frame.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        command: GatewayCommandKind,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            protocol: GATEWAY_PROTOCOL_VERSION.to_owned(),
            id: id.into(),
            command,
            payload,
        }
    }

    /// Typed payload access for `SubmitTask`.
    pub fn submit_task(&self) -> Result<GatewaySubmitTask, String> {
        serde_json::from_value(self.payload.clone())
            .map_err(|err| format!("invalid SubmitTask payload: {err}"))
    }

    /// Protocol tag + id sanity shared by every handler.
    pub fn validate(&self) -> Result<(), String> {
        if self.protocol != GATEWAY_PROTOCOL_VERSION {
            return Err(format!("unsupported protocol: {}", self.protocol));
        }
        if self.id.trim().is_empty() || self.id.len() > 128 {
            return Err("command id must be 1..=128 chars".to_owned());
        }
        if let GatewayCommandKind::SubmitTask = self.command {
            let task: GatewaySubmitTask = serde_json::from_value(self.payload.clone())
                .map_err(|err| format!("invalid SubmitTask payload: {err}"))?;
            let title = task.title.trim();
            if title.is_empty() || title.len() > 256 {
                return Err("title must be 1..=256 chars".to_owned());
            }
        }
        Ok(())
    }
}

/// Events the daemon broadcasts (mirrors `GatewayEventKind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GatewayEventKind {
    /// A durable task left `queued` for `executing`.
    TaskStarted,
    /// A durable task reached a terminal state.
    TaskCompleted,
    /// A policy-gated tool was invoked (dry-run or live).
    ToolInvoked,
    /// An evidence row was recorded and hash-chained.
    EvidenceRecorded,
    /// A policy decision escalated to human review.
    PolicyRequired,
    /// The browser agent paused on a CAPTCHA/bot wall (human modal).
    CaptchaDetected,
    /// The browser agent paused on a login wall (human modal).
    AuthRequired,
    /// An ephemeral computer-use VM session changed (launch/destroy/status).
    ComputerVmChanged,
    /// A fresh ephemeral-VM screen frame (VNC stream event).
    ComputerVmFrame,
    /// Heartbeat answer (carries `{ sequence }`).
    Pong,
    /// Structured failure (carries `{ code, message, inReplyTo }`).
    Error,
    /// Scheduler listing answer (carries `{ schedules, inReplyTo }`).
    SchedulesListed,
}

/// Typed error payload for `Error` events.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayErrorPayload {
    /// Stable error code (e.g. `bad_frame`).
    pub code: String,
    /// Human-readable detail.
    pub message: String,
    /// Command id being answered, if any.
    pub in_reply_to: Option<String>,
}

/// One event frame: `{ protocol, event, payload, taskId?, at }`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayEvent {
    /// Protocol tag (always `talos-gateway/1`).
    pub protocol: String,
    /// Event kind.
    pub event: GatewayEventKind,
    /// Kind-specific payload.
    pub payload: serde_json::Value,
    /// Owning task, when the event is task-scoped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    /// RFC 3339 emission time.
    pub at: String,
}

impl GatewayEvent {
    /// Build a versioned event frame stamped with the current time.
    #[must_use]
    pub fn new(
        event: GatewayEventKind,
        payload: serde_json::Value,
        task_id: Option<String>,
    ) -> Self {
        Self {
            protocol: GATEWAY_PROTOCOL_VERSION.to_owned(),
            event,
            payload,
            task_id,
            at: talos_common::now_iso(),
        }
    }

    /// Error event addressed to one command id.
    #[must_use]
    pub fn error(code: &str, message: &str, in_reply_to: Option<&str>) -> Self {
        Self::new(
            GatewayEventKind::Error,
            serde_json::json!({
                "code": code,
                "message": message,
                "inReplyTo": in_reply_to,
            }),
            None,
        )
    }

    /// Heartbeat pong answering a `Ping` sequence.
    #[must_use]
    pub fn pong(sequence: u64) -> Self {
        Self::new(
            GatewayEventKind::Pong,
            serde_json::json!({ "sequence": sequence }),
            None,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_round_trip() {
        let cmd = GatewayCommand::new(
            "cmd-1",
            GatewayCommandKind::SubmitTask,
            serde_json::json!({ "title": "probe", "priority": "normal" }),
        );
        assert!(cmd.validate().is_ok());
        let task = cmd.submit_task().expect("parses");
        assert_eq!(task.title, "probe");
        let back: GatewayCommand =
            serde_json::from_value(serde_json::to_value(&cmd).expect("json")).expect("decode");
        assert_eq!(back.id, "cmd-1");
        assert_eq!(back.protocol, GATEWAY_PROTOCOL_VERSION);
    }

    #[test]
    fn empty_title_rejected() {
        let cmd = GatewayCommand::new(
            "cmd-2",
            GatewayCommandKind::SubmitTask,
            serde_json::json!({ "title": "   " }),
        );
        assert!(cmd.validate().is_err());
    }

    #[test]
    fn wrong_protocol_rejected() {
        let mut cmd = GatewayCommand::new("cmd-3", GatewayCommandKind::Ping, serde_json::json!({}));
        cmd.protocol = "bogus/9".to_owned();
        assert!(cmd.validate().is_err());
    }

    #[test]
    fn hitl_events_serialize_like_typescript() {
        for kind in [
            GatewayEventKind::CaptchaDetected,
            GatewayEventKind::AuthRequired,
        ] {
            let event = GatewayEvent::new(
                kind,
                serde_json::json!({
                    "interventionId": "hitl-1",
                    "taskId": "t-1",
                    "kind": "captcha_detected",
                    "reason": "cloudflare-challenge",
                }),
                Some("t-1".to_owned()),
            );
            let raw = serde_json::to_value(&event).expect("json");
            assert_eq!(raw["event"], format!("{kind:?}"));
            assert_eq!(raw["taskId"], "t-1");
        }
    }

    #[test]
    fn event_serializes_with_timestamp() {
        let event = GatewayEvent::new(
            GatewayEventKind::TaskCompleted,
            serde_json::json!({ "taskId": "t-1", "status": "completed" }),
            Some("t-1".to_owned()),
        );
        let raw = serde_json::to_value(&event).expect("json");
        assert_eq!(raw["protocol"], GATEWAY_PROTOCOL_VERSION);
        assert_eq!(raw["event"], "TaskCompleted");
        let back: GatewayEvent = serde_json::from_value(raw).expect("decode");
        assert_eq!(back.task_id.as_deref(), Some("t-1"));
    }

    #[test]
    fn wire_shape_is_camel_case_like_typescript() {
        // The TS Zod schemas read `taskId` / `inReplyTo`; the Rust mirror
        // must emit exactly those keys, never snake_case.
        let event = GatewayEvent::new(
            GatewayEventKind::TaskStarted,
            serde_json::json!({ "taskId": "t-9" }),
            Some("t-9".to_owned()),
        );
        let raw = serde_json::to_string(&event).expect("json");
        assert!(
            raw.contains("\"taskId\":\"t-9\""),
            "camelCase taskId: {raw}"
        );
        assert!(!raw.contains("task_id"), "no snake_case leak: {raw}");
        let error = GatewayEvent::error("bad_frame", "nope", Some("cmd-1"));
        let raw = serde_json::to_string(&error).expect("json");
        assert!(
            raw.contains("\"inReplyTo\":\"cmd-1\""),
            "camelCase inReplyTo: {raw}"
        );
        // Inbound TS payloads decode: `taskId`, not `task_id`.
        let cancel: GatewayCancelTask =
            serde_json::from_value(serde_json::json!({ "taskId": "t-9" })).expect("decodes");
        assert_eq!(cancel.task_id, "t-9");
    }
}
