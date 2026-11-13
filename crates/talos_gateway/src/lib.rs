//! Talos gateway daemon: persistent headless background process.
//!
//! The gateway owns the exclusive SQLite WAL lock (via the existing v0.4.0
//! encryption envelope) and serves local clients — the Tauri shell, the UI
//! bridge, the CLI — over a loopback WebSocket (`ws://127.0.0.1:8080/talos`).
//! Closing the desktop window never stops this process; tasks, heartbeats,
//! and ledger writes continue uninterrupted.
//!
//! Wire protocol mirrors `@talos/contracts` `gateway.ts` exactly:
//! JSON-RPC-like `Command` frames up (`SubmitTask`, `CancelTask`,
//! `ApprovePolicy`, `Ping`, `GetStatus`, `ForwardIpc`) and `Event` frames
//! down (`TaskStarted`, `TaskCompleted`, `ToolInvoked`, `EvidenceRecorded`,
//! `PolicyRequired`, `Pong`, `Error`).

pub mod auth;
pub mod protocol;
pub mod server;
pub mod state;

pub use protocol::{
    GatewayApprovePolicy, GatewayCancelTask, GatewayCommand, GatewayCommandKind,
    GatewayErrorPayload, GatewayEvent, GatewayEventKind, GatewayForwardIpc, GatewaySubmitTask,
    ALLOWED_ORIGINS, GATEWAY_HEARTBEAT_INTERVAL_MS, GATEWAY_PID_FILE_NAME, GATEWAY_PORT,
    GATEWAY_PROTOCOL_VERSION, GATEWAY_ROUTE, GATEWAY_SUBPROTOCOL, GATEWAY_TOKEN_FILE_NAME,
    GATEWAY_TOKEN_SUBPROTOCOL_PREFIX,
};
pub use server::{broadcast_event, serve, serve_ephemeral, ServerHandle};
pub use state::{GatewayState, GatewayStatus};
