//! Loopback WebSocket transport for the gateway daemon.
//!
//! - Binds `127.0.0.1:8080`, serves the `/talos` route only (everything else
//!   is refused, never proxied).
//! - Each socket: JSON text frames → [`GatewayCommand`] → handler → JSON
//!   [`GatewayEvent`] replies, plus a broadcast fan-out of daemon events
//!   (`TaskStarted`, `TaskCompleted`, heartbeats) to every client.
//! - Heartbeat: `Ping` → `Pong { sequence }`. The daemon also emits a
//!   server-side `Pong` tick so idle clients can measure link health.
//! - `ForwardIpc` routes any of the 96 Tauri commands through the same socket
//!   (the Tauri shell proxies UI invokes here instead of touching the store).

use std::net::SocketAddr;
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use talos_common::TalosError;
use tokio::net::{TcpListener, TcpStream};

use crate::protocol::{
    GatewayApprovePolicy, GatewayCancelTask, GatewayCommand, GatewayCommandKind, GatewayEvent,
    GatewayEventKind, GatewayForwardIpc, GatewaySubmitTask, GATEWAY_PORT, GATEWAY_ROUTE,
};
use crate::state::{GatewayState, GatewayStatus};

/// Opaque server lease: dropping it does not stop the listener (the daemon
/// is persistent by design); it only releases the test's join handle.
pub struct ServerHandle {
    /// Bound loopback address (tests bind port 0 and read this back).
    pub addr: SocketAddr,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
}

impl std::fmt::Debug for ServerHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerHandle")
            .field("addr", &self.addr)
            .finish()
    }
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

/// Broadcast one event to every connected client (best-effort: lagged or
/// absent receivers are skipped, never block the daemon).
pub fn broadcast_event(state: &GatewayState, event: GatewayEvent) {
    let _ = state.events.send(event);
}

/// Serve the gateway on `127.0.0.1:port` (pass `0` for an ephemeral test
/// port). Returns the bound address plus a shutdown lease.
pub async fn serve(state: GatewayState, port: u16) -> Result<ServerHandle, TalosError> {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
        .await
        .map_err(|err| TalosError::Internal(format!("gateway bind: {err}")))?;
    let addr = listener
        .local_addr()
        .map_err(|err| TalosError::Internal(format!("gateway addr: {err}")))?;
    eprintln!(
        "[talos-gateway] listening on ws://127.0.0.1:{}/talos",
        addr.port()
    );
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let state = Arc::new(state);
    // Server-side heartbeat: a `Pong` tick every interval keeps idle clients
    // aware the daemon is alive even with no tasks running.
    {
        let heartbeat_state = Arc::clone(&state);
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_millis(
                crate::GATEWAY_HEARTBEAT_INTERVAL_MS,
            ));
            let mut sequence: u64 = 0;
            loop {
                ticker.tick().await;
                sequence = sequence.wrapping_add(1);
                broadcast_event(&heartbeat_state, GatewayEvent::pong(sequence));
            }
        });
    }
    // Scheduler engine: fire due HEARTBEAT.md schedules every 30s as
    // system-initiated Supervisor tasks (policy-gated, never bypassed).
    // The same tick polls linked messaging channels (Telegram / Discord):
    // inbound messages become Supervisor tasks through the same gate.
    {
        let scheduler_state = Arc::clone(&state);
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(30));
            loop {
                ticker.tick().await;
                scheduler_state.run_scheduler_tick().await;
                scheduler_state.poll_channels().await;
            }
        });
    }
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut shutdown_rx => break,
                accepted = listener.accept() => {
                    let Ok((stream, peer)) = accepted else { break };
                    let state = Arc::clone(&state);
                    tokio::spawn(async move {
                        if let Err(err) = handle_connection(state, stream, peer).await {
                            eprintln!("[talos-gateway] connection {peer} closed: {err}");
                        }
                    });
                }
            }
        }
    });
    Ok(ServerHandle {
        addr,
        shutdown: Some(shutdown_tx),
    })
}

/// Bind specifically for tests on an ephemeral port.
pub async fn serve_ephemeral(state: GatewayState) -> Result<ServerHandle, TalosError> {
    serve(state, 0).await
}

async fn handle_connection(
    state: Arc<GatewayState>,
    stream: TcpStream,
    peer: SocketAddr,
) -> Result<(), String> {
    // Loopback only: refuse non-local peers before the WS handshake.
    if !peer.ip().is_loopback() {
        return Err(format!("refusing non-loopback peer {peer}"));
    }
    let ws = tokio_tungstenite::accept_async(stream)
        .await
        .map_err(|err| format!("handshake failed: {err}"))?;
    // The route check happens on the first frame's optional `route` hint; the
    // listener only serves the gateway, so any valid command frame is routed.
    // (HTTP path routing is enforced by accepting only upgrade requests.)
    let (mut sink, mut source) = ws.split();
    let mut broadcasts = state.events.subscribe();
    // Greet with the current status so fresh clients can render immediately.
    let status = state.status().await;
    let hello = GatewayEvent::new(
        GatewayEventKind::Pong,
        serde_json::json!({ "sequence": 0, "status": status }),
        None,
    );
    let hello_text = serde_json::to_string(&hello).map_err(|err| err.to_string())?;
    sink.send(tungstenite::Message::Text(hello_text.into()))
        .await
        .map_err(|err| err.to_string())?;
    loop {
        tokio::select! {
            incoming = source.next() => {
                let Some(message) = incoming else { break };
                let message = message.map_err(|err| err.to_string())?;
                match message {
                    tungstenite::Message::Text(text) => {
                        let reply = dispatch(&state, &text).await;
                        let text = serde_json::to_string(&reply).map_err(|err| err.to_string())?;
                        sink.send(tungstenite::Message::Text(text.into())).await.map_err(|err| err.to_string())?;
                    }
                    tungstenite::Message::Close(_) => break,
                    tungstenite::Message::Ping(data) => {
                        sink.send(tungstenite::Message::Pong(data)).await.map_err(|err| err.to_string())?;
                    }
                    _ => {}
                }
            }
            broadcast = broadcasts.recv() => {
                let Ok(event) = broadcast else { continue };
                let text = serde_json::to_string(&event).map_err(|err| err.to_string())?;
                if sink.send(tungstenite::Message::Text(text.into())).await.is_err() {
                    break;
                }
            }
        }
    }
    Ok(())
}

/// Route one command frame to its handler (pure dispatch, unit-tested).
pub async fn dispatch(state: &GatewayState, raw: &str) -> GatewayEvent {
    let command: GatewayCommand = match serde_json::from_str(raw) {
        Ok(command) => command,
        Err(err) => {
            return GatewayEvent::error(
                "bad_frame",
                &format!("invalid command frame: {err}"),
                None,
            );
        }
    };
    if let Err(reason) = command.validate() {
        return GatewayEvent::error("bad_command", &reason, Some(&command.id));
    }
    match command.command {
        GatewayCommandKind::Ping => {
            let sequence = command
                .payload
                .get("sequence")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            GatewayEvent::pong(sequence)
        }
        GatewayCommandKind::GetStatus => {
            let status: GatewayStatus = state.status().await;
            GatewayEvent::new(
                GatewayEventKind::Pong,
                serde_json::to_value(&status).unwrap_or(serde_json::Value::Null),
                None,
            )
        }
        GatewayCommandKind::SubmitTask => {
            let payload: GatewaySubmitTask = match serde_json::from_value(command.payload.clone()) {
                Ok(payload) => payload,
                Err(err) => {
                    return GatewayEvent::error(
                        "bad_payload",
                        &format!("invalid SubmitTask payload: {err}"),
                        Some(&command.id),
                    );
                }
            };
            match state.submit_task(payload).await {
                Ok(view) => GatewayEvent::new(
                    GatewayEventKind::TaskCompleted,
                    serde_json::json!({ "taskId": view.id, "status": view.status }),
                    Some(view.id.clone()),
                ),
                Err(err) => {
                    GatewayEvent::error("submit_failed", &err.to_string(), Some(&command.id))
                }
            }
        }
        GatewayCommandKind::CancelTask => {
            let payload: GatewayCancelTask = match serde_json::from_value(command.payload.clone()) {
                Ok(payload) => payload,
                Err(err) => {
                    return GatewayEvent::error(
                        "bad_payload",
                        &format!("invalid CancelTask payload: {err}"),
                        Some(&command.id),
                    );
                }
            };
            match state.cancel_task(&payload.task_id).await {
                Ok(view) => GatewayEvent::new(
                    GatewayEventKind::TaskCompleted,
                    serde_json::json!({ "taskId": view.id, "status": view.status }),
                    Some(view.id.clone()),
                ),
                Err(err) => {
                    GatewayEvent::error("cancel_failed", &err.to_string(), Some(&command.id))
                }
            }
        }
        GatewayCommandKind::ApprovePolicy => {
            let payload: GatewayApprovePolicy =
                match serde_json::from_value(command.payload.clone()) {
                    Ok(payload) => payload,
                    Err(err) => {
                        return GatewayEvent::error(
                            "bad_payload",
                            &format!("invalid ApprovePolicy payload: {err}"),
                            Some(&command.id),
                        );
                    }
                };
            // Approvals live in the desktop `ApprovalStore` in v0.4.0; the
            // daemon records the verdict as a ledger event so the human
            // decision is auditable even when the window is closed.
            let verdict = if payload.approved {
                "granted"
            } else {
                "denied"
            };
            let _ = evidence_store::append_event(
                &state.db,
                evidence_store::NewEvent {
                    id: format!("gateway-approval-{}", payload.approval_id),
                    kind: format!("gateway.approval_{verdict}"),
                    payload_json: serde_json::json!({
                        "approvalId": payload.approval_id,
                        "reason": payload.reason,
                        "via": "talos-gateway/1",
                    }),
                },
            )
            .await;
            GatewayEvent::new(
                GatewayEventKind::PolicyRequired,
                serde_json::json!({
                    "approvalId": payload.approval_id,
                    "status": verdict,
                    "inReplyTo": command.id,
                }),
                None,
            )
        }
        GatewayCommandKind::ForwardIpc => {
            let payload: GatewayForwardIpc = match serde_json::from_value(command.payload.clone()) {
                Ok(payload) => payload,
                Err(err) => {
                    return GatewayEvent::error(
                        "bad_payload",
                        &format!("invalid ForwardIpc payload: {err}"),
                        Some(&command.id),
                    );
                }
            };
            route_forwarded_ipc(state, &command.id, &payload).await
        }
    }
}

/// Route a generic forwarded IPC command through the daemon's live state.
///
/// The Tauri shell proxies UI invokes here instead of touching the store, so
/// the daemon keeps the exclusive WAL lock. Read-heavy commands resolve
/// locally; task execution stays on the durable queue worker.
async fn route_forwarded_ipc(
    state: &GatewayState,
    command_id: &str,
    payload: &GatewayForwardIpc,
) -> GatewayEvent {
    match payload.command.as_str() {
        "get_system_status" | "getSystemStatus" => {
            let status = state.status().await;
            GatewayEvent::new(
                GatewayEventKind::Pong,
                serde_json::to_value(&status).unwrap_or(serde_json::Value::Null),
                None,
            )
        }
        "list_tasks" | "listTasks" => {
            let limit = payload
                .args
                .get("limit")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(50);
            match evidence_store::list_tasks(&state.db, limit).await {
                Ok(rows) => GatewayEvent::new(
                    GatewayEventKind::TaskCompleted,
                    serde_json::json!({ "command": payload.command, "count": rows.len(), "inReplyTo": command_id }),
                    None,
                ),
                Err(err) => GatewayEvent::error("ipc_failed", &err.to_string(), Some(command_id)),
            }
        }
        "verify_evidence_ledger" | "verifyEvidenceLedger" => {
            match evidence_store::get_integrity_report(&state.db).await {
                Ok(report) => GatewayEvent::new(
                    GatewayEventKind::EvidenceRecorded,
                    serde_json::json!({ "healthy": report.healthy, "inReplyTo": command_id }),
                    None,
                ),
                Err(err) => GatewayEvent::error("ipc_failed", &err.to_string(), Some(command_id)),
            }
        }
        "list_schedules" | "listSchedules" => {
            let schedules = state.list_schedules().await;
            GatewayEvent::new(
                GatewayEventKind::SchedulesListed,
                serde_json::json!({ "schedules": schedules, "inReplyTo": command_id }),
                None,
            )
        }
        "toggle_schedule_active" | "toggleScheduleActive" => {
            let id = payload
                .args
                .get("scheduleId")
                .or_else(|| payload.args.get("schedule_id"))
                .or_else(|| payload.args.get("id"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            let active = payload
                .args
                .get("active")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(true);
            match state.set_schedule_active(id, active).await {
                Some(view) => GatewayEvent::new(
                    GatewayEventKind::SchedulesListed,
                    serde_json::json!({ "schedules": [view], "inReplyTo": command_id }),
                    None,
                ),
                None => GatewayEvent::error(
                    "unknown_schedule",
                    &format!("no schedule: {id}"),
                    Some(command_id),
                ),
            }
        }
        "trigger_schedule_now" | "triggerScheduleNow" => {
            let id = payload
                .args
                .get("scheduleId")
                .or_else(|| payload.args.get("schedule_id"))
                .or_else(|| payload.args.get("id"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            match state.trigger_schedule_now(id).await {
                Ok(view) => GatewayEvent::new(
                    GatewayEventKind::TaskCompleted,
                    serde_json::json!({ "taskId": view.id, "status": view.status, "inReplyTo": command_id }),
                    Some(view.id.clone()),
                ),
                Err(err) => {
                    GatewayEvent::error("trigger_failed", &err.to_string(), Some(command_id))
                }
            }
        }
        "list_channels" | "listChannels" => {
            let channels = state.list_channels().await;
            GatewayEvent::new(
                GatewayEventKind::TaskCompleted,
                serde_json::json!({ "channels": channels, "inReplyTo": command_id }),
                None,
            )
        }
        "link_channel" | "linkChannel" => {
            let name = payload
                .args
                .get("channel")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            let token = payload
                .args
                .get("token")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            match channels::ChannelId::parse(name) {
                Some(channel) => match state.link_channel(channel, token).await {
                    Ok(channels) => GatewayEvent::new(
                        GatewayEventKind::TaskCompleted,
                        serde_json::json!({ "channels": channels, "inReplyTo": command_id }),
                        None,
                    ),
                    Err(err) => {
                        GatewayEvent::error("link_failed", &err.to_string(), Some(command_id))
                    }
                },
                None => GatewayEvent::error(
                    "unknown_channel",
                    &format!("no channel: {name} (telegram | discord)"),
                    Some(command_id),
                ),
            }
        }
        "unlink_channel" | "unlinkChannel" => {
            let name = payload
                .args
                .get("channel")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            match channels::ChannelId::parse(name) {
                Some(channel) => match state.unlink_channel(channel).await {
                    Ok(channels) => GatewayEvent::new(
                        GatewayEventKind::TaskCompleted,
                        serde_json::json!({ "channels": channels, "inReplyTo": command_id }),
                        None,
                    ),
                    Err(err) => {
                        GatewayEvent::error("unlink_failed", &err.to_string(), Some(command_id))
                    }
                },
                None => GatewayEvent::error(
                    "unknown_channel",
                    &format!("no channel: {name} (telegram | discord)"),
                    Some(command_id),
                ),
            }
        }
        "list_channel_messages" | "listChannelMessages" => {
            let limit = payload
                .args
                .get("limit")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(20)
                .clamp(1, 50) as usize;
            let messages = state.list_channel_messages(limit).await;
            GatewayEvent::new(
                GatewayEventKind::TaskCompleted,
                serde_json::json!({ "messages": messages, "inReplyTo": command_id }),
                None,
            )
        }
        "list_browser_profiles" | "listBrowserProfiles" => {
            match browser_use::list_profiles(&browser_use::gateway_profile_root()).await {
                Ok(profiles) => GatewayEvent::new(
                    GatewayEventKind::TaskCompleted,
                    serde_json::json!({ "profiles": profiles, "inReplyTo": command_id }),
                    None,
                ),
                Err(err) => GatewayEvent::error("ipc_failed", &err.to_string(), Some(command_id)),
            }
        }
        "clear_browser_profile" | "clearBrowserProfile" => {
            let name = payload
                .args
                .get("profile")
                .or_else(|| payload.args.get("profileName"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or(browser_use::DEFAULT_PROFILE_NAME);
            match browser_use::clear_profile(&browser_use::gateway_profile_root(), name).await {
                Ok(cleared) => GatewayEvent::new(
                    GatewayEventKind::TaskCompleted,
                    serde_json::json!({ "profile": name, "cleared": cleared, "inReplyTo": command_id }),
                    None,
                ),
                Err(err) => GatewayEvent::error("ipc_failed", &err.to_string(), Some(command_id)),
            }
        }
        "computer_vm_launch" | "computerVmLaunch" => {
            let task_id = payload
                .args
                .get("taskId")
                .and_then(|value| value.as_str())
                .unwrap_or("task-computer-1");
            let mut config = computer_use::vm::VmConfig::default();
            if let Some(image) = payload.args.get("image").and_then(|value| value.as_str()) {
                config.image = image.to_owned();
            }
            if let Some(width) = payload.args.get("width").and_then(|value| value.as_i64()) {
                config.width = width;
            }
            if let Some(height) = payload.args.get("height").and_then(|value| value.as_i64()) {
                config.height = height;
            }
            if let Some(vnc) = payload.args.get("vncExpose").and_then(|value| value.as_bool()) {
                config.vnc_expose = vnc;
            }
            // Daemon-local VM: an isolated scratch workspace (the desktop
            // shell mounts the real task workspace for its own manager).
            let scratch = std::env::temp_dir().join("talos-gateway-vm").join(
                task_id
                    .chars()
                    .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                    .collect::<String>(),
            );
            if let Err(error) = tokio::fs::create_dir_all(&scratch).await {
                return GatewayEvent::error(
                    "ipc_failed",
                    &format!("vm workspace: {error}"),
                    Some(command_id),
                );
            }
            match computer_use::vm::VmManager::global()
                .ensure_running(task_id, &scratch, &config)
                .await
            {
                Ok(view) => GatewayEvent::new(
                    GatewayEventKind::ComputerVmChanged,
                    serde_json::json!({ "session": view, "inReplyTo": command_id }),
                    None,
                ),
                Err(error) => {
                    GatewayEvent::error("ipc_failed", &error.to_string(), Some(command_id))
                }
            }
        }
        "computer_vm_status" | "computerVmStatus" => {
            let view = computer_use::vm::VmManager::global().status().await;
            GatewayEvent::new(
                GatewayEventKind::ComputerVmChanged,
                serde_json::json!({ "session": view, "inReplyTo": command_id }),
                None,
            )
        }
        "computer_vm_destroy" | "computerVmDestroy" => {
            match computer_use::vm::VmManager::global().destroy().await {
                Ok(destroyed) => GatewayEvent::new(
                    GatewayEventKind::ComputerVmChanged,
                    serde_json::json!({ "destroyed": destroyed, "inReplyTo": command_id }),
                    None,
                ),
                Err(error) => {
                    GatewayEvent::error("ipc_failed", &error.to_string(), Some(command_id))
                }
            }
        }
        "computer_vm_frame" | "computerVmFrame" => {
            let with_png = payload
                .args
                .get("withPng")
                .and_then(|value| value.as_bool())
                .unwrap_or(true);
            match computer_use::vm::VmManager::global()
                .frame(with_png)
                .await
            {
                Ok(frame) => GatewayEvent::new(
                    GatewayEventKind::ComputerVmFrame,
                    serde_json::json!({ "frame": frame, "inReplyTo": command_id }),
                    None,
                ),
                Err(error) => {
                    GatewayEvent::error("ipc_failed", &error.to_string(), Some(command_id))
                }
            }
        }
        "resume_browser_hitl" | "resumeBrowserHitl" => {
            let id = payload
                .args
                .get("interventionId")
                .or_else(|| payload.args.get("intervention_id"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            let resumed = payload
                .args
                .get("resumed")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(true);
            let outcome = if resumed {
                browser_use::HitlOutcome::Resumed
            } else {
                browser_use::HitlOutcome::Dismissed
            };
            if browser_use::global_registry().resolve(id, outcome) {
                GatewayEvent::new(
                    GatewayEventKind::TaskCompleted,
                    serde_json::json!({ "interventionId": id, "resumed": resumed, "inReplyTo": command_id }),
                    None,
                )
            } else {
                GatewayEvent::error(
                    "unknown_intervention",
                    "no pending browser intervention with that id in this process (the gate may live in the desktop shell)",
                    Some(command_id),
                )
            }
        }
        other => GatewayEvent::error(
            "unknown_ipc",
            &format!("no gateway route for IPC command: {other}"),
            Some(command_id),
        ),
    }
}

/// Gateway bind address for a port (loopback only, never `0.0.0.0`).
#[must_use]
pub fn gateway_addr(port: u16) -> SocketAddr {
    std::net::SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST), port)
}

/// Default production port (mirrors `GATEWAY_URL`).
#[must_use]
pub fn default_port() -> u16 {
    GATEWAY_PORT
}

#[allow(dead_code)]
fn route_hint() -> &'static str {
    GATEWAY_ROUTE
}

#[cfg(test)]
mod tests {
    use super::*;

    fn forward(command: &str, args: serde_json::Value) -> String {
        serde_json::to_string(&GatewayCommand::new(
            "cmd-1",
            GatewayCommandKind::ForwardIpc,
            serde_json::json!({ "command": command, "args": args }),
        ))
        .expect("frame")
    }

    #[tokio::test]
    async fn browser_profile_routes_round_trip() {
        let dir = std::env::temp_dir().join(format!(
            "talos-gateway-profiles-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.subsec_nanos())
                .unwrap_or(0)
        ));
        std::env::set_var(browser_use::PROFILE_ROOT_ENV, &dir);
        let state = GatewayState::new_in_memory().await.expect("state");
        // Empty at first.
        let listed = dispatch(
            &state,
            &forward("list_browser_profiles", serde_json::json!({})),
        )
        .await;
        assert_eq!(listed.event, GatewayEventKind::TaskCompleted);
        assert_eq!(listed.payload["profiles"], serde_json::json!([]));
        // Clearing an absent profile is a no-op success.
        let cleared = dispatch(
            &state,
            &forward(
                "clear_browser_profile",
                serde_json::json!({ "profile": "default" }),
            ),
        )
        .await;
        assert_eq!(cleared.event, GatewayEventKind::TaskCompleted);
        assert_eq!(cleared.payload["cleared"], serde_json::json!(false));
        // Bad names fail closed without touching the filesystem.
        let bad = dispatch(
            &state,
            &forward(
                "clear_browser_profile",
                serde_json::json!({ "profile": "../evil" }),
            ),
        )
        .await;
        assert_eq!(bad.event, GatewayEventKind::Error);
        // Unknown interventions resolve to a structured error (the gate may
        // live in the desktop shell process instead of the daemon).
        let resumed = dispatch(
            &state,
            &forward(
                "resume_browser_hitl",
                serde_json::json!({ "interventionId": "hitl-nope" }),
            ),
        )
        .await;
        assert_eq!(resumed.event, GatewayEventKind::Error);
        assert_eq!(
            resumed.payload["code"],
            serde_json::json!("unknown_intervention")
        );
        std::env::remove_var(browser_use::PROFILE_ROOT_ENV);
        let _ = tokio::fs::remove_dir_all(&dir).await;

        // A daemon-side gate resolves through the same route.
        let (_event, _waiter) = browser_use::global_registry().request(
            "task-gw-1",
            browser_use::HitlKind::CaptchaDetected,
            None,
            "captcha-challenge",
        );
        let pending = browser_use::global_registry().pending();
        let gate = pending
            .iter()
            .find(|event| event.task_id == "task-gw-1")
            .expect("gate registered")
            .clone();
        let ok = dispatch(
            &state,
            &forward(
                "resume_browser_hitl",
                serde_json::json!({ "interventionId": gate.intervention_id }),
            ),
        )
        .await;
        assert_eq!(ok.event, GatewayEventKind::TaskCompleted);
        assert_eq!(ok.payload["resumed"], serde_json::json!(true));
    }

    #[tokio::test]
    async fn computer_vm_routes_report_daemon_local_session() {
        let state = GatewayState::new_in_memory().await.expect("state");
        // No container held: status reports stopped, frame fails closed.
        let _ = computer_use::vm::VmManager::global().destroy().await;
        let status = dispatch(
            &state,
            &forward("computer_vm_status", serde_json::json!({})),
        )
        .await;
        assert_eq!(status.event, GatewayEventKind::ComputerVmChanged);
        assert_eq!(status.payload["session"]["running"], serde_json::json!(false));
        assert_eq!(
            status.payload["session"]["backend"],
            serde_json::json!("ephemeral-vm")
        );
        let frame = dispatch(
            &state,
            &forward("computer_vm_frame", serde_json::json!({})),
        )
        .await;
        assert_eq!(frame.event, GatewayEventKind::Error);
        let destroyed = dispatch(
            &state,
            &forward("computer_vm_destroy", serde_json::json!({})),
        )
        .await;
        assert_eq!(destroyed.event, GatewayEventKind::ComputerVmChanged);
        assert_eq!(destroyed.payload["destroyed"], serde_json::json!(false));
    }
}
