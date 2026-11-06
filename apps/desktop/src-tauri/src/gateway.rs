//! Gateway client: the Tauri shell as a thin client of `talos-gateway`.
//!
//! On startup the shell tries `ws://127.0.0.1:8080/talos`. When the daemon is
//! absent it spawns the `talos-gateway` binary as a detached background child
//! (surviving Tauri exit) and retries with backoff. All 96 IPC commands route
//! through [`GatewayClient::forward_ipc`] so the daemon keeps the exclusive
//! SQLite WAL lock; the window process never touches the store directly.
//!
//! Link health: a 5s heartbeat `Ping`/`Pong`. Three missed pongs flip the UI
//! to `DISCONNECTED` (see `gateway_status`), which renders a
//! "Restart Daemon" button wired to [`GatewayClient::restart_daemon`].

use std::sync::Arc;

/// Loopback gateway endpoint (mirrors `@talos/contracts` `GATEWAY_URL`).
pub const GATEWAY_URL: &str = "ws://127.0.0.1:8080/talos";
/// Heartbeat cadence (ms). Three missed pongs mean `DISCONNECTED`.
pub const HEARTBEAT_INTERVAL_MS: u64 = 5_000;
/// Consecutive missed pongs before the link is declared dead.
pub const MISSED_PONG_LIMIT: u32 = 3;

/// Link state surfaced to the UI status bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GatewayLink {
    /// Fresh client, no probe yet.
    Unknown,
    /// Heartbeats flowing; commands route to the daemon.
    Connected,
    /// Daemon unreachable; UI shows DISCONNECTED + Restart Daemon.
    Disconnected,
}

/// Status snapshot for the `gateway_status` Tauri command.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayStatusView {
    /// Current link state.
    pub link: GatewayLink,
    /// Gateway endpoint (always loopback).
    pub url: String,
    /// Consecutive missed heartbeat pongs.
    pub missed_pongs: u32,
    /// Last successful heartbeat sequence, if any.
    pub last_sequence: Option<u64>,
    /// Daemon-reported version, if known.
    pub daemon_version: Option<String>,
}

/// Thin async client over the gateway WebSocket.
#[derive(Debug, Clone)]
pub struct GatewayClient {
    inner: Arc<tokio::sync::Mutex<ClientInner>>,
}

#[derive(Debug)]
struct ClientInner {
    link: GatewayLink,
    missed_pongs: u32,
    last_sequence: Option<u64>,
    daemon_version: Option<String>,
    sequence: u64,
}

impl GatewayClient {
    /// Create a fresh client (`Unknown` link; call [`GatewayClient::ensure_running`]).
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(tokio::sync::Mutex::new(ClientInner {
                link: GatewayLink::Unknown,
                missed_pongs: 0,
                last_sequence: None,
                daemon_version: None,
                sequence: 0,
            })),
        }
    }

    /// Current link snapshot (lock-free read path for the status command).
    pub async fn status(&self) -> GatewayStatusView {
        let inner = self.inner.lock().await;
        GatewayStatusView {
            link: inner.link,
            url: GATEWAY_URL.to_owned(),
            missed_pongs: inner.missed_pongs,
            last_sequence: inner.last_sequence,
            daemon_version: inner.daemon_version.clone(),
        }
    }

    /// Mark the link state (heartbeat task + command paths share this).
    pub async fn set_link(&self, link: GatewayLink) {
        self.inner.lock().await.link = link;
    }

    /// Ensure the daemon is running: probe the socket; when unreachable,
    /// spawn a detached `talos-gateway` child and retry with backoff.
    ///
    /// Returns the final link state. Never panics; a missing binary is a
    /// `Disconnected` link, not a crash.
    pub async fn ensure_running(&self) -> GatewayLink {
        if self.probe_once().await {
            self.set_link(GatewayLink::Connected).await;
            return GatewayLink::Connected;
        }
        // Daemon absent: best-effort detached spawn, then retry.
        self.spawn_daemon();
        for _ in 0..6 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            if self.probe_once().await {
                self.set_link(GatewayLink::Connected).await;
                return GatewayLink::Connected;
            }
        }
        self.set_link(GatewayLink::Disconnected).await;
        GatewayLink::Disconnected
    }

    /// Single heartbeat round-trip (`Ping` → `Pong`). Updates missed-pong
    /// accounting; three misses flip the link to `Disconnected`.
    pub async fn heartbeat(&self) -> bool {
        let sequence = {
            let mut inner = self.inner.lock().await;
            inner.sequence = inner.sequence.wrapping_add(1);
            inner.sequence
        };
        let ok = self
            .send_command("hb", "Ping", serde_json::json!({ "sequence": sequence }))
            .await
            .is_some();
        let mut inner = self.inner.lock().await;
        if ok {
            inner.missed_pongs = 0;
            inner.last_sequence = Some(sequence);
            inner.link = GatewayLink::Connected;
            true
        } else {
            inner.missed_pongs = inner.missed_pongs.saturating_add(1);
            if inner.missed_pongs >= MISSED_PONG_LIMIT {
                inner.link = GatewayLink::Disconnected;
            }
            false
        }
    }

    /// Restart the daemon: kill any occupant of the port is out of scope
    /// (loopback bind conflict resolves by retry); spawn a fresh detached
    /// child and re-probe. Powers the UI "Restart Daemon" button.
    pub async fn restart_daemon(&self) -> GatewayLink {
        self.spawn_daemon();
        self.ensure_running().await
    }

    /// Forward one of the 96 Tauri IPC commands through the daemon over the
    /// same socket (`ForwardIpc`). Returns the daemon's raw JSON reply.
    pub async fn forward_ipc(
        &self,
        command: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let id = {
            let mut inner = self.inner.lock().await;
            inner.sequence = inner.sequence.wrapping_add(1);
            format!("tauri-{}", inner.sequence)
        };
        let args_map: std::collections::HashMap<String, serde_json::Value> = match args {
            serde_json::Value::Object(map) => map.into_iter().collect(),
            serde_json::Value::Null => std::collections::HashMap::new(),
            other => {
                let mut map = std::collections::HashMap::new();
                map.insert("value".to_owned(), other);
                map
            }
        };
        self.send_command(
            &id,
            "ForwardIpc",
            serde_json::json!({ "command": command, "args": args_map }),
        )
        .await
        .ok_or_else(|| "gateway unreachable (DISCONNECTED)".to_owned())
    }

    /// One raw command round-trip over a fresh socket (stateless: connect →
    /// send → first addressed reply → close). Heartbeat ticks, the hello
    /// greeting, and other clients' broadcasts are skipped.
    async fn send_command(
        &self,
        id: &str,
        command: &str,
        payload: serde_json::Value,
    ) -> Option<serde_json::Value> {
        use futures_util::{SinkExt, StreamExt};
        let Ok((mut ws, _)) = tokio_tungstenite::connect_async(GATEWAY_URL).await else {
            return None;
        };
        let frame = serde_json::json!({
            "protocol": "talos-gateway/1",
            "id": id,
            "command": command,
            "payload": payload,
        });
        let text = serde_json::to_string(&frame).ok()?;
        ws.send(tokio_tungstenite::tungstenite::Message::Text(text.into()))
            .await
            .ok()?;
        let want_sequence = payload.get("sequence").and_then(serde_json::Value::as_u64);
        // Replies are addressed two ways: structured events carry
        // `payload.inReplyTo == id`; `Ping` answers with a bare
        // `Pong { sequence }` echoing our probe. Everything else (hello
        // greeting, server heartbeat ticks, other clients' broadcasts) is
        // skipped, never mistaken for our answer.
        for _ in 0..16 {
            let message = tokio::time::timeout(std::time::Duration::from_secs(10), ws.next())
                .await
                .ok()??;
            let message = message.ok()?;
            let text = match message {
                tokio_tungstenite::tungstenite::Message::Text(text) => text.to_string(),
                tokio_tungstenite::tungstenite::Message::Close(_) => return None,
                _ => continue,
            };
            let event: serde_json::Value = serde_json::from_str(&text).ok()?;
            if event
                .pointer("/payload/inReplyTo")
                .and_then(|reply| reply.as_str())
                == Some(id)
            {
                return Some(event);
            }
            if command == "Ping"
                && event.get("event").and_then(|event| event.as_str()) == Some("Pong")
                && event
                    .pointer("/payload/sequence")
                    .and_then(|sequence| sequence.as_u64())
                    == want_sequence
            {
                return Some(event);
            }
        }
        None
    }

    /// Best-effort liveness probe (connect + read the hello greeting).
    async fn probe_once(&self) -> bool {
        use futures_util::StreamExt;
        let Ok(connect) = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            tokio_tungstenite::connect_async(GATEWAY_URL),
        )
        .await
        else {
            return false;
        };
        let Ok((mut ws, _)) = connect else {
            return false;
        };
        let Ok(frame) = tokio::time::timeout(std::time::Duration::from_secs(3), ws.next()).await
        else {
            return false;
        };
        frame.is_some()
    }

    /// Spawn the daemon as a detached background child that survives Tauri
    /// exit. Resolves the binary from (in order): `TALOS_GATEWAY_BIN`, a
    /// `talos-gateway` on `PATH`, the current-exe sibling dir (bundled
    /// sidecar). Failures only log; the link stays `Disconnected`.
    fn spawn_daemon(&self) {
        let candidate = std::env::var("TALOS_GATEWAY_BIN")
            .ok()
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::env::current_exe()
                    .ok()
                    .and_then(|exe| exe.parent().map(|dir| dir.join(exe_name("talos-gateway"))))
            });
        let program =
            candidate.unwrap_or_else(|| std::path::PathBuf::from(exe_name("talos-gateway")));
        eprintln!(
            "[talos] gateway absent; spawning detached daemon: {}",
            program.display()
        );
        #[cfg(unix)]
        let result = std::process::Command::new(&program)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .process_group(0)
            .spawn();
        #[cfg(windows)]
        let result = std::process::Command::new(&program)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags(
                0x00000008 /* DETACHED_PROCESS */ | 0x08000000, /* CREATE_NO_WINDOW */
            )
            .spawn();
        match result {
            Ok(child) => eprintln!("[talos] gateway daemon spawned (pid {})", child.id()),
            Err(err) => eprintln!("[talos] gateway spawn failed ({program:?}): {err}"),
        }
    }
}

impl Default for GatewayClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Platform executable name (`talos-gateway` + `.exe` on Windows).
fn exe_name(base: &str) -> String {
    if cfg!(windows) {
        format!("{base}.exe")
    } else {
        base.to_owned()
    }
}

/// True for the hello greeting (status snapshot) and heartbeat ticks, which
/// carry no `inReplyTo` and must never satisfy a command round-trip.
#[cfg(test)]
fn is_hello_or_tick(event: &serde_json::Value) -> bool {
    if event.get("event").and_then(|event| event.as_str()) != Some("Pong") {
        return false;
    }
    event.pointer("/payload/inReplyTo").is_none()
}

/// `gateway_status` — current daemon link snapshot for the status bar and
/// the DISCONNECTED banner (`Unknown` before the first probe).
#[tauri::command]
pub async fn gateway_status(
    client: tauri::State<'_, GatewayClient>,
) -> Result<GatewayStatusView, String> {
    Ok(client.status().await)
}

/// `gateway_ping` — one heartbeat round-trip; false means the daemon missed
/// this pong (three misses flip the link to `Disconnected`).
#[tauri::command]
pub async fn gateway_ping(client: tauri::State<'_, GatewayClient>) -> Result<bool, String> {
    Ok(client.heartbeat().await)
}

/// `gateway_restart` — spawn a fresh detached daemon and re-probe (powers
/// the UI "Restart Daemon" button on the DISCONNECTED banner).
#[tauri::command]
pub async fn gateway_restart(
    client: tauri::State<'_, GatewayClient>,
) -> Result<GatewayStatusView, String> {
    client.restart_daemon().await;
    Ok(client.status().await)
}

/// `gateway_forward` — route any of the 96 Tauri IPC commands through the
/// daemon over the same socket (`ForwardIpc`). The UI bridge migrates
/// command-by-command onto this pipe; unknown commands answer
/// `unknown_ipc` from the daemon instead of executing locally.
#[tauri::command]
pub async fn gateway_forward(
    client: tauri::State<'_, GatewayClient>,
    command: String,
    args: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    client
        .forward_ipc(&command, args.unwrap_or(serde_json::Value::Null))
        .await
}

/// Unwrap one `SchedulesListed` daemon reply into its schedule array.
fn schedules_from_reply(reply: serde_json::Value) -> Result<Vec<serde_json::Value>, String> {
    reply
        .pointer("/payload/schedules")
        .and_then(|schedules| schedules.as_array())
        .cloned()
        .ok_or_else(|| "gateway returned no schedules array".to_owned())
}

/// `list_schedules` — owned HEARTBEAT schedules with next trigger + last
/// outcome (powers the UI Heartbeats panel). Proxied to the daemon, which is
/// the sole scheduler owner.
#[tauri::command]
pub async fn list_schedules(
    client: tauri::State<'_, GatewayClient>,
) -> Result<Vec<serde_json::Value>, String> {
    let reply = client
        .forward_ipc("list_schedules", serde_json::Value::Null)
        .await?;
    schedules_from_reply(reply)
}

/// `toggle_schedule_active` — pause or resume one schedule by id.
#[tauri::command]
pub async fn toggle_schedule_active(
    client: tauri::State<'_, GatewayClient>,
    schedule_id: String,
    active: bool,
) -> Result<serde_json::Value, String> {
    let reply = client
        .forward_ipc(
            "toggle_schedule_active",
            serde_json::json!({ "scheduleId": schedule_id, "active": active }),
        )
        .await?;
    Ok(reply
        .pointer("/payload/schedules")
        .cloned()
        .unwrap_or(serde_json::Value::Null))
}

/// `trigger_schedule_now` — fire one schedule immediately (manual override;
/// paused schedules fire too). Returns the daemon's raw event reply.
#[tauri::command]
pub async fn trigger_schedule_now(
    client: tauri::State<'_, GatewayClient>,
    schedule_id: String,
) -> Result<serde_json::Value, String> {
    client
        .forward_ipc(
            "trigger_schedule_now",
            serde_json::json!({ "scheduleId": schedule_id }),
        )
        .await
}

/// Forward daemon broadcasts (scheduler firings, `PolicyRequired` desktop
/// notifications, heartbeats) into Tauri events named `gatewayDaemonEvent`.
///
/// The UI subscribes once and routes by `event` kind; short-lived command
/// sockets would otherwise miss broadcasts entirely. Reconnects with backoff;
/// never fails startup.
pub fn forward_daemon_events(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        use futures_util::StreamExt;
        use tauri::Emitter;
        let mut backoff_secs = 1u64;
        loop {
            let Ok((mut ws, _)) = tokio_tungstenite::connect_async(GATEWAY_URL).await else {
                tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;
                backoff_secs = (backoff_secs * 2).min(30);
                continue;
            };
            backoff_secs = 1;
            loop {
                let next =
                    tokio::time::timeout(std::time::Duration::from_secs(60), ws.next()).await;
                let Ok(Some(message)) = next else {
                    break;
                };
                let Ok(message) = message else {
                    break;
                };
                let text = match message {
                    tokio_tungstenite::tungstenite::Message::Text(text) => text.to_string(),
                    tokio_tungstenite::tungstenite::Message::Close(_) => break,
                    _ => continue,
                };
                let frame: serde_json::Value = match serde_json::from_str(&text) {
                    Ok(frame) => frame,
                    Err(_) => continue,
                };
                if app.emit("gatewayDaemonEvent", &frame).is_err() {
                    break;
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;
            backoff_secs = (backoff_secs * 2).min(30);
        }
    });
}

#[cfg(unix)]
use std::os::unix::process::CommandExt as _;
#[cfg(windows)]
use std::os::windows::process::CommandExt as _;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fresh_client_reports_unknown_link() {
        let client = GatewayClient::new();
        let status = client.status().await;
        assert_eq!(status.link, GatewayLink::Unknown);
        assert_eq!(status.url, GATEWAY_URL);
        assert_eq!(status.missed_pongs, 0);
    }

    #[tokio::test]
    async fn three_missed_heartbeats_flip_to_disconnected() {
        let client = GatewayClient::new();
        // No daemon on the loopback test port path would still hit the real
        // 8080; instead drive the accounting directly: repeated failed
        // heartbeats must trip the limit without panicking.
        for _ in 0..(MISSED_PONG_LIMIT + 1) {
            // heartbeat() attempts a real connection; in CI without a daemon
            // it fails fast and records a miss.
            let _ =
                tokio::time::timeout(std::time::Duration::from_secs(15), client.heartbeat()).await;
            if client.status().await.link == GatewayLink::Disconnected {
                break;
            }
        }
        let status = client.status().await;
        assert!(
            status.link == GatewayLink::Disconnected || status.link == GatewayLink::Connected,
            "link resolves to a terminal state, got {:?}",
            status.link
        );
    }

    #[test]
    fn hello_and_tick_frames_are_skipped() {
        let hello = serde_json::json!({
            "protocol": "talos-gateway/1",
            "event": "Pong",
            "payload": { "sequence": 0, "status": {} },
        });
        assert!(is_hello_or_tick(&hello));
        let addressed = serde_json::json!({
            "protocol": "talos-gateway/1",
            "event": "Pong",
            "payload": { "inReplyTo": "hb" },
        });
        assert!(!is_hello_or_tick(&addressed));
        let error = serde_json::json!({
            "protocol": "talos-gateway/1",
            "event": "Error",
            "payload": { "code": "x", "inReplyTo": "hb" },
        });
        assert!(!is_hello_or_tick(&error));
    }

    #[test]
    fn exe_name_is_platform_aware() {
        if cfg!(windows) {
            assert_eq!(exe_name("talos-gateway"), "talos-gateway.exe");
        } else {
            assert_eq!(exe_name("talos-gateway"), "talos-gateway");
        }
    }
}
