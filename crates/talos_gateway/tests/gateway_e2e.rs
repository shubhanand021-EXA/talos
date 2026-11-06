//! Gateway E2E: startup, WebSocket acceptance, command routing, event
//! broadcast, and a full submit → `task_completed` round-trip.

use futures_util::{SinkExt, StreamExt};
use talos_gateway::{
    serve_ephemeral, GatewayCommand, GatewayCommandKind, GatewayEventKind, GatewayState,
};

async fn connect(
    addr: std::net::SocketAddr,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let url = format!("ws://127.0.0.1:{}/talos", addr.port());
    let (ws, _) = tokio_tungstenite::connect_async(&url)
        .await
        .expect("gateway accepts WebSocket connections");
    ws
}

async fn next_event(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> talos_gateway::GatewayEvent {
    let message = ws.next().await.expect("frame").expect("message");
    let text = match message {
        tungstenite::Message::Text(text) => text.to_string(),
        other => panic!("expected text frame, got {other:?}"),
    };
    serde_json::from_str(&text).expect("event decodes")
}

#[tokio::test]
async fn gateway_starts_and_greets_with_status() {
    let state = GatewayState::new_in_memory().await.expect("state");
    let handle = serve_ephemeral(state).await.expect("serves");
    let mut ws = connect(handle.addr).await;
    // First frame is the hello `Pong` carrying a status snapshot.
    let hello = next_event(&mut ws).await;
    assert_eq!(hello.event, GatewayEventKind::Pong);
    assert_eq!(hello.protocol, talos_gateway::GATEWAY_PROTOCOL_VERSION);
}

#[tokio::test]
async fn ping_pong_heartbeat_round_trip() {
    let state = GatewayState::new_in_memory().await.expect("state");
    let handle = serve_ephemeral(state).await.expect("serves");
    let mut ws = connect(handle.addr).await;
    let _ = next_event(&mut ws).await; // hello
    let cmd = GatewayCommand::new(
        "ping-1",
        GatewayCommandKind::Ping,
        serde_json::json!({ "sequence": 41 }),
    );
    ws.send(tungstenite::Message::Text(
        serde_json::to_string(&cmd).expect("json").into(),
    ))
    .await
    .expect("sends");
    // The server-side heartbeat Pong tick may interleave; wait for OUR pong.
    for _ in 0..10 {
        let reply = next_event(&mut ws).await;
        if reply.event == GatewayEventKind::Pong && reply.payload["sequence"] == 41 {
            return;
        }
    }
    panic!("pong for sequence 41 never arrived");
}

#[tokio::test]
async fn e2e_submit_task_receives_task_completed() {
    let state = GatewayState::new_in_memory().await.expect("state");
    let handle = serve_ephemeral(state).await.expect("serves");
    let mut ws = connect(handle.addr).await;
    let _ = next_event(&mut ws).await; // hello
    let cmd = GatewayCommand::new(
        "submit-1",
        GatewayCommandKind::SubmitTask,
        serde_json::json!({ "title": "gateway e2e probe" }),
    );
    ws.send(tungstenite::Message::Text(
        serde_json::to_string(&cmd).expect("json").into(),
    ))
    .await
    .expect("sends");
    // The daemon broadcasts TaskStarted, then answers TaskCompleted.
    // The server-side heartbeat Pong may interleave; skip it.
    let mut saw_started = false;
    let mut saw_completed = false;
    for _ in 0..10 {
        let event = next_event(&mut ws).await;
        match event.event {
            GatewayEventKind::TaskStarted => saw_started = true,
            GatewayEventKind::TaskCompleted => {
                saw_completed = true;
                assert!(event.task_id.is_some());
                break;
            }
            GatewayEventKind::Pong => {}
            other => panic!("unexpected event: {other:?}"),
        }
    }
    assert!(saw_started || saw_completed, "task lifecycle observed");
    assert!(saw_completed, "task_completed received");
}

#[tokio::test]
async fn bad_frames_answer_structured_errors() {
    let state = GatewayState::new_in_memory().await.expect("state");
    let handle = serve_ephemeral(state).await.expect("serves");
    let mut ws = connect(handle.addr).await;
    let _ = next_event(&mut ws).await; // hello
    ws.send(tungstenite::Message::Text("not json".into()))
        .await
        .expect("sends");
    // Skip interleaved heartbeat ticks; the error reply is deterministic.
    for _ in 0..10 {
        let reply = next_event(&mut ws).await;
        if reply.event == GatewayEventKind::Error {
            assert_eq!(reply.payload["code"], "bad_frame");
            return;
        }
    }
    panic!("bad_frame error never arrived");
}

#[tokio::test]
async fn unknown_ipc_command_is_rejected() {
    let state = GatewayState::new_in_memory().await.expect("state");
    let handle = serve_ephemeral(state).await.expect("serves");
    let mut ws = connect(handle.addr).await;
    let _ = next_event(&mut ws).await; // hello
    let cmd = GatewayCommand::new(
        "fwd-1",
        GatewayCommandKind::ForwardIpc,
        serde_json::json!({ "command": "no_such_command_xyz", "args": {} }),
    );
    ws.send(tungstenite::Message::Text(
        serde_json::to_string(&cmd).expect("json").into(),
    ))
    .await
    .expect("sends");
    // Skip interleaved heartbeat ticks; the rejection is deterministic.
    for _ in 0..10 {
        let reply = next_event(&mut ws).await;
        if reply.event == GatewayEventKind::Error {
            assert_eq!(reply.payload["code"], "unknown_ipc");
            return;
        }
    }
    panic!("unknown_ipc error never arrived");
}
