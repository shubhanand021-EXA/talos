//! Ephemeral VM backend: per-task Linux desktop containers for computer-use.
//!
//! Containment contract (Pentagon Architecture): the agent NEVER touches the
//! host OS. Every live computer action executes inside a fresh container that
//! the Gateway spawns on demand, drives over `docker exec` (screenshots via
//! `import`, input via `xdotool`), and destroys afterwards:
//!
//! - Exactly one bind-mount: the task workspace at `/workspace`, **read-only**
//!   (never the host root, never daemon directories).
//! - No network by default (`--network none`); the loopback VNC publisher is
//!   opt-in per session (`vnc_expose`), bound to `127.0.0.1` only.
//! - Hardening flags on every spawn: `--rm`, `--cap-drop=ALL`,
//!   `--security-opt=no-new-privileges`, `--pids-limit=256`, memory/CPU caps.
//! - The writable layer dies with the container (`--rm`); durable artifacts
//!   leave only through [`VmManager::export_file`] into the evidence store.
//! - Docker absent/unreachable fails closed (`vm_unavailable`); nothing falls
//!   back to the host.
//!
//! [`VmTransport`] abstracts the runtime so tests run hermetically on
//! [`MockTransport`] (no daemon, deterministic frames, recorded input).
//! Production uses [`DockerTransport`] (Linux/Windows Docker; macOS runs the
//! same CLI against a Lima-managed Docker host — see `tools/vm/README.md`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use talos_common::{Result, TalosError};

use crate::driver::DriverAction;

fn err(msg: impl Into<String>) -> TalosError {
    TalosError::Storage(msg.into())
}

/// Read-only workspace mount point inside the guest.
pub const GUEST_WORKSPACE: &str = "/workspace";
/// Writable agent home inside the guest (discarded with the container).
pub const GUEST_HOME: &str = "/home/agent";
/// Export source directory: the only guest tree `export_file` may copy out.
pub const GUEST_EXPORT_DIR: &str = "/home/agent/output";
/// Default container image (override with `TALOS_VM_IMAGE`).
pub const DEFAULT_IMAGE: &str = "talos/desktop-vm:0.1";
/// Environment override for the container image.
pub const IMAGE_ENV: &str = "TALOS_VM_IMAGE";
/// Largest frame accepted from the guest (2 MiB; bigger ⇒ `vm_frame_too_large`).
pub const MAX_FRAME_BYTES: usize = 2 * 1024 * 1024;
/// Largest artifact `export_file` will copy out (5 MiB).
pub const MAX_EXPORT_BYTES: usize = 5 * 1024 * 1024;
/// Spawn budget (covers a cold image pull).
pub const SPAWN_TIMEOUT: Duration = Duration::from_secs(180);
/// Per-action budget inside the guest.
pub const EXEC_TIMEOUT: Duration = Duration::from_secs(30);

/// Ephemeral-VM session configuration (mirrors `@talos/contracts` `VmConfig`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VmConfig {
    /// Container image (default `talos/desktop-vm:0.1`, `TALOS_VM_IMAGE` wins).
    pub image: String,
    /// Container name prefix (`talos-vm`).
    pub container_name_prefix: String,
    /// Guest desktop width in pixels.
    pub width: i64,
    /// Guest desktop height in pixels.
    pub height: i64,
    /// Container memory cap in MiB.
    pub memory_mb: u64,
    /// Container CPU quota (`1.0` = one core).
    pub cpus: String,
    /// Publish x11vnc on loopback for human viewing (default false).
    pub vnc_expose: bool,
    /// Idle seconds before auto-destroy (0 = never).
    pub idle_timeout_secs: u64,
}

impl Default for VmConfig {
    fn default() -> Self {
        Self {
            image: std::env::var(IMAGE_ENV)
                .ok()
                .filter(|raw| !raw.trim().is_empty())
                .unwrap_or_else(|| DEFAULT_IMAGE.to_owned()),
            container_name_prefix: "talos-vm".to_owned(),
            width: 1280,
            height: 800,
            memory_mb: 1024,
            cpus: "1.0".to_owned(),
            vnc_expose: false,
            idle_timeout_secs: 600,
        }
    }
}

impl VmConfig {
    /// Validate bounds (fail closed before any container exists).
    pub fn validate(&self) -> Result<()> {
        if self.image.trim().is_empty() || self.image.len() > 256 {
            return Err(err("vm image must be 1..=256 chars"));
        }
        if !(640..=3840).contains(&self.width) || !(480..=2160).contains(&self.height) {
            return Err(err("vm resolution must be 640..=3840 x 480..=2160"));
        }
        if !(256..=8192).contains(&self.memory_mb) {
            return Err(err("vm memory must be 256..=8192 MiB"));
        }
        if self.idle_timeout_secs > 3600 {
            return Err(err("vm idle timeout must be 0..=3600s"));
        }
        Ok(())
    }
}

/// Live session view (status responses, never secrets).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VmSessionView {
    /// A container is currently held.
    pub running: bool,
    /// Container id, when running.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container_id: Option<String>,
    /// Container name, when running.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container_name: Option<String>,
    /// Loopback VNC URL for human viewing, when exposed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vnc_url: Option<String>,
    /// Guest desktop resolution.
    pub width: i64,
    /// Guest desktop resolution.
    pub height: i64,
    /// RFC 3339 session start, when running.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    /// Backend wire value (`ephemeral-vm`).
    pub backend: String,
}

/// One streamed frame (mirrors `@talos/contracts` `VncStreamEvent`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VmFrame {
    /// Monotonic frame sequence for this session.
    pub seq: u64,
    /// `sha256:` over the PNG bytes.
    pub hash: String,
    /// Frame width in pixels.
    pub width: i64,
    /// Frame height in pixels.
    pub height: i64,
    /// Base64 PNG (`None` when the caller only needs metadata).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub png_base64: Option<String>,
    /// RFC 3339 capture time.
    pub at: String,
}

/// Container identity returned by a spawn.
#[derive(Debug, Clone)]
pub struct ContainerInfo {
    /// Runtime container id.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Published loopback VNC port, when exposed.
    pub vnc_port: Option<u16>,
}

/// One `exec` result.
#[derive(Debug, Clone)]
pub struct ExecOutput {
    /// Process exit status.
    pub status: Option<i32>,
    /// Bounded stdout.
    pub stdout: String,
    /// Bounded stderr (truncated, never secrets — guest has none).
    pub stderr: String,
}

/// Container specification handed to the transport.
#[derive(Debug, Clone)]
pub struct ContainerSpec {
    /// Container name.
    pub name: String,
    /// Image reference.
    pub image: String,
    /// Host workspace dir → mounted at `/workspace:ro`.
    pub workspace_host: PathBuf,
    /// Guest resolution.
    pub width: i64,
    /// Guest resolution.
    pub height: i64,
    /// Memory cap MiB.
    pub memory_mb: u64,
    /// CPU quota string.
    pub cpus: String,
    /// Publish loopback VNC.
    pub vnc_expose: bool,
}

/// Pluggable container runtime (Docker CLI in production, mock in tests).
#[async_trait::async_trait]
pub trait VmTransport: Send + Sync + std::fmt::Debug {
    /// Spawn a hardened container from the spec.
    async fn spawn(&self, spec: &ContainerSpec) -> Result<ContainerInfo>;
    /// Run argv inside the container (no shell).
    async fn exec(&self, container: &str, argv: &[String]) -> Result<ExecOutput>;
    /// Copy one guest file to a host path.
    async fn copy_out(&self, container: &str, guest: &str, host: &Path) -> Result<u64>;
    /// Capture the guest root window as PNG bytes.
    async fn fetch_png(&self, container: &str) -> Result<Vec<u8>>;
    /// Freeze all guest processes (`docker pause`).
    async fn pause(&self, container: &str) -> Result<()>;
    /// Unfreeze guest processes.
    async fn unpause(&self, container: &str) -> Result<()>;
    /// Force-remove the container (idempotent).
    async fn remove(&self, container: &str) -> Result<()>;
    /// True when the container exists and is running.
    async fn exists(&self, container: &str) -> Result<bool>;
}

/// Translate one resolved action into `xdotool` argv (pure, no I/O).
///
/// Coordinates are guest pixels; text travels as a single argv element after
/// `--` so no shell is ever involved.
pub fn xdotool_argv(action: &DriverAction) -> Result<Vec<String>> {
    let num = |v: i64| v.to_string();
    match action {
        DriverAction::MoveTo { x, y } => Ok(vec!["mousemove".to_owned(), num(*x), num(*y)]),
        DriverAction::Click { x, y, button } => {
            let code = match button.to_ascii_lowercase().as_str() {
                "left" => "1",
                "middle" => "2",
                "right" => "3",
                other => return Err(err(format!("unsupported mouse button: {other}"))),
            };
            Ok(vec![
                "mousemove".to_owned(),
                num(*x),
                num(*y),
                "click".to_owned(),
                code.to_owned(),
            ])
        }
        DriverAction::DoubleClick { x, y } => Ok(vec![
            "mousemove".to_owned(),
            num(*x),
            num(*y),
            "click".to_owned(),
            "--repeat".to_owned(),
            "2".to_owned(),
            "1".to_owned(),
        ]),
        DriverAction::Drag { x, y, x2, y2 } => Ok(vec![
            "mousemove".to_owned(),
            num(*x),
            num(*y),
            "mousedown".to_owned(),
            "1".to_owned(),
            "mousemove".to_owned(),
            num(*x2),
            num(*y2),
            "mouseup".to_owned(),
            "1".to_owned(),
        ]),
        DriverAction::TypeText { text } => {
            if text.is_empty() || text.len() > 4096 {
                return Err(err("text must be 1..=4096 chars"));
            }
            Ok(vec!["type".to_owned(), "--".to_owned(), text.clone()])
        }
        DriverAction::Hotkey { keys } => {
            let chord = translate_chord(keys)?;
            Ok(vec!["key".to_owned(), chord])
        }
        DriverAction::Scroll {
            x,
            y,
            direction,
            pixels,
        } => {
            // Button 4/5/6/7 = up/down/left/right; one click ≈ 120px.
            let button = match direction.to_ascii_lowercase().as_str() {
                "up" => "4",
                "down" => "5",
                "left" => "6",
                "right" => "7",
                other => return Err(err(format!("unsupported scroll direction: {other}"))),
            };
            let repeats = ((*pixels).clamp(1, 10_000) / 120).clamp(1, 10);
            let mut argv = Vec::new();
            if let (Some(x), Some(y)) = (x, y) {
                argv.extend(["mousemove".to_owned(), num(*x), num(*y)]);
            }
            argv.extend([
                "click".to_owned(),
                "--repeat".to_owned(),
                repeats.to_string(),
                button.to_owned(),
            ]);
            Ok(argv)
        }
        DriverAction::Activate => Err(err("activate has no xdotool mapping (focus via click)")),
    }
}

/// Map a Talos `+`-separated chord to xdotool `key` syntax.
fn translate_chord(keys: &str) -> Result<String> {
    if keys.trim().is_empty() || keys.len() > 64 {
        return Err(err("keys must be 1..=64 chars"));
    }
    // Leak-free static F-key table (no allocation per key).
    const FKEYS: [&str; 20] = [
        "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12", "F13", "F14",
        "F15", "F16", "F17", "F18", "F19", "F20",
    ];
    let mut out: Vec<String> = Vec::new();
    for token in keys.split('+') {
        let token = token.trim();
        if token.is_empty() {
            return Err(err("hotkey chord has an empty token"));
        }
        let lower = token.to_ascii_lowercase();
        let mapped: String = match lower.as_str() {
            "ctrl" | "control" => "ctrl".to_owned(),
            "shift" => "shift".to_owned(),
            "alt" => "alt".to_owned(),
            "meta" | "cmd" | "command" | "win" | "windows" | "super" => "Super_L".to_owned(),
            "enter" | "return" => "Return".to_owned(),
            "tab" => "Tab".to_owned(),
            "esc" | "escape" => "Escape".to_owned(),
            "del" | "delete" => "Delete".to_owned(),
            "backspace" => "BackSpace".to_owned(),
            "space" => "space".to_owned(),
            "up" | "uparrow" => "Up".to_owned(),
            "down" | "downarrow" => "Down".to_owned(),
            "left" | "leftarrow" => "Left".to_owned(),
            "right" | "rightarrow" => "Right".to_owned(),
            "home" => "Home".to_owned(),
            "end" => "End".to_owned(),
            "pageup" => "Page_Up".to_owned(),
            "pagedown" => "Page_Down".to_owned(),
            token if token.starts_with('f') => {
                let number: u8 = token[1..]
                    .parse()
                    .map_err(|_| err(format!("unsupported hotkey token: {token}")))?;
                if !(1..=20).contains(&number) {
                    return Err(err(format!("unsupported hotkey token: {token}")));
                }
                FKEYS[(number - 1) as usize].to_owned()
            }
            _ => {
                let mut chars = token.chars();
                match (chars.next(), chars.next()) {
                    (Some(single), None) => single.to_string(),
                    _ => return Err(err(format!("unsupported hotkey token: {token}"))),
                }
            }
        };
        out.push(mapped);
    }
    Ok(out.join("+"))
}

/// Screenshot argv: root-window PNG to stdout (no shell, no files).
#[must_use]
pub fn screenshot_argv() -> Vec<String> {
    vec![
        "import".to_owned(),
        "-window".to_owned(),
        "root".to_owned(),
        "png:-".to_owned(),
    ]
}

/// Validate a guest export path: relative, bounded, no escapes.
///
/// Returns the absolute guest source under [`GUEST_EXPORT_DIR`].
pub fn resolve_export_guest(rel: &str) -> Result<String> {
    if rel.trim().is_empty() || rel.len() > 256 {
        return Err(err("export path must be 1..=256 chars"));
    }
    if rel.starts_with('/') {
        return Err(err("export path must be relative to the agent output dir"));
    }
    let mut depth = 0i32;
    for part in rel.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                depth -= 1;
                if depth < 0 {
                    return Err(err("export path escapes the output dir"));
                }
            }
            _ => {
                if part.len() > 128 {
                    return Err(err("export path segment too long"));
                }
                depth += 1;
            }
        }
    }
    Ok(format!("{GUEST_EXPORT_DIR}/{rel}"))
}

/// Validate a host workspace root for the read-only mount (fail closed).
///
/// Refuses `/`, empty, and missing directories — the container must never
/// see the host root as a fallback.
pub async fn resolve_host_workspace(root: &Path) -> Result<PathBuf> {
    let metadata = tokio::fs::metadata(root)
        .await
        .map_err(|error| err(format!("vm workspace missing: {error}")))?;
    if !metadata.is_dir() {
        return Err(err("vm workspace root is not a directory"));
    }
    let canonical = tokio::fs::canonicalize(root)
        .await
        .map_err(|error| err(format!("vm workspace unresolvable: {error}")))?;
    let mut components = canonical.components();
    // Exactly one normal component (e.g. `/tmp`) with nothing else ⇒ the
    // filesystem root (or a drive root on Windows) is refused.
    let mut normal_count = 0;
    for component in components.by_ref() {
        if matches!(component, std::path::Component::Normal(_)) {
            normal_count += 1;
        }
    }
    if normal_count == 0 {
        return Err(err("vm workspace must not be the filesystem root"));
    }
    Ok(canonical)
}

/// Check PNG magic bytes (the guest must return a real frame).
pub fn is_png(bytes: &[u8]) -> bool {
    bytes.len() >= 8 && bytes[0..8] == [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]
}

/// Production transport: the local Docker CLI (Linux/Windows Docker Desktop;
/// macOS via a Lima-managed Docker host — see `tools/vm/README.md`).
#[derive(Debug, Default)]
pub struct DockerTransport {
    /// `docker` binary (default `docker`, `TALOS_DOCKER_BIN` wins).
    pub binary: Option<String>,
}

impl DockerTransport {
    fn program(&self) -> String {
        self.binary.clone().unwrap_or_else(|| {
            std::env::var("TALOS_DOCKER_BIN")
                .ok()
                .filter(|raw| !raw.trim().is_empty())
                .unwrap_or_else(|| "docker".to_owned())
        })
    }
}

#[async_trait::async_trait]
impl VmTransport for DockerTransport {
    async fn spawn(&self, spec: &ContainerSpec) -> Result<ContainerInfo> {
        // Probe the daemon first: a missing Docker fails closed here, never
        // as a half-created container later.
        let probe = self
            .run(&["info".to_owned()], Duration::from_secs(15), 4096)
            .await
            .map_err(|_| err("vm_unavailable: docker daemon unreachable"))?;
        if probe.status != Some(0) {
            return Err(err("vm_unavailable: docker daemon unreachable"));
        }
        let mut argv = vec![
            "run".to_owned(),
            "-d".to_owned(),
            "--rm".to_owned(),
            "--name".to_owned(),
            spec.name.clone(),
            "--cap-drop=ALL".to_owned(),
            "--security-opt=no-new-privileges".to_owned(),
            "--pids-limit=256".to_owned(),
            "--memory".to_owned(),
            format!("{}m", spec.memory_mb),
            "--cpus".to_owned(),
            spec.cpus.clone(),
            "--network".to_owned(),
            if spec.vnc_expose {
                "bridge".to_owned()
            } else {
                "none".to_owned()
            },
        ];
        if spec.vnc_expose {
            // Loopback-only VNC publisher for human viewing.
            argv.extend(["-p".to_owned(), "127.0.0.1::5900".to_owned()]);
        }
        argv.extend([
            "-v".to_owned(),
            format!("{}:{GUEST_WORKSPACE}:ro", spec.workspace_host.display()),
            "-e".to_owned(),
            "DISPLAY=:0".to_owned(),
            spec.image.clone(),
        ]);
        let out = self
            .run(&argv, SPAWN_TIMEOUT, 4096)
            .await
            .map_err(|_| err("vm_unavailable: docker spawn failed"))?;
        if out.status != Some(0) {
            return Err(err(format!(
                "vm_spawn_failed: {}",
                out.stderr.chars().take(256).collect::<String>()
            )));
        }
        let id = out.stdout.trim().to_owned();
        if id.is_empty() {
            return Err(err("vm_spawn_failed: empty container id"));
        }
        let vnc_port = if spec.vnc_expose {
            self.docker_port(&id).await?
        } else {
            None
        };
        Ok(ContainerInfo {
            id,
            name: spec.name.clone(),
            vnc_port,
        })
    }

    async fn exec(&self, container: &str, argv: &[String]) -> Result<ExecOutput> {
        let mut full = vec![
            "exec".to_owned(),
            container.to_owned(),
            "xdotool".to_owned(),
        ];
        full.extend(argv.iter().cloned());
        let out = self.run(&full, EXEC_TIMEOUT, 8192).await?;
        if out.status != Some(0) {
            return Err(err(format!(
                "vm exec failed: {}",
                out.stderr.chars().take(256).collect::<String>()
            )));
        }
        Ok(out)
    }

    async fn copy_out(&self, container: &str, guest: &str, host: &Path) -> Result<u64> {
        if let Some(parent) = host.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|error| err(format!("vm export staging failed: {error}")))?;
        }
        let out = self
            .run(
                &[
                    "cp".to_owned(),
                    format!("{container}:{guest}"),
                    host.to_string_lossy().into_owned(),
                ],
                Duration::from_secs(60),
                4096,
            )
            .await?;
        if out.status != Some(0) {
            return Err(err("export_missing: guest artifact not found"));
        }
        let bytes = tokio::fs::metadata(host)
            .await
            .map_err(|error| err(format!("vm export stat failed: {error}")))?
            .len();
        if bytes > MAX_EXPORT_BYTES as u64 {
            let _ = tokio::fs::remove_file(host).await;
            return Err(err("export_rejected: artifact exceeds 5 MiB"));
        }
        Ok(bytes)
    }

    async fn fetch_png(&self, container: &str) -> Result<Vec<u8>> {
        use tokio::io::AsyncReadExt as _;
        let mut full = vec!["exec".to_owned(), container.to_owned()];
        full.extend(screenshot_argv());
        let mut child = tokio::process::Command::new(self.program())
            .args(&full)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|_| err("vm_unavailable: docker spawn failed"))?;
        let mut stdout_pipe = child.stdout.take();
        let exit = tokio::select! {
            result = child.wait() => result.map_err(|error| err(format!("vm frame failed: {error}")))?,
            _ = tokio::time::sleep(EXEC_TIMEOUT) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                return Err(err("vm frame timed out"));
            }
        };
        let mut png = Vec::new();
        if let Some(pipe) = stdout_pipe.as_mut() {
            let _ = pipe
                .take(MAX_FRAME_BYTES as u64 + 1)
                .read_to_end(&mut png)
                .await;
        }
        if !exit.success() {
            return Err(err("vm frame capture failed"));
        }
        if png.len() > MAX_FRAME_BYTES {
            return Err(err("vm_frame_too_large: frame exceeds 2 MiB"));
        }
        if !is_png(&png) {
            return Err(err("vm frame is not a PNG"));
        }
        Ok(png)
    }

    async fn pause(&self, container: &str) -> Result<()> {
        let out = self
            .run(
                &["pause".to_owned(), container.to_owned()],
                Duration::from_secs(15),
                1024,
            )
            .await?;
        if out.status != Some(0) {
            return Err(err("vm pause failed"));
        }
        Ok(())
    }

    async fn unpause(&self, container: &str) -> Result<()> {
        let out = self
            .run(
                &["unpause".to_owned(), container.to_owned()],
                Duration::from_secs(15),
                1024,
            )
            .await?;
        if out.status != Some(0) {
            return Err(err("vm unpause failed"));
        }
        Ok(())
    }

    async fn remove(&self, container: &str) -> Result<()> {
        // Idempotent: a missing container is already the desired end state.
        let _ = self
            .run(
                &["rm".to_owned(), "-f".to_owned(), container.to_owned()],
                Duration::from_secs(30),
                1024,
            )
            .await;
        Ok(())
    }

    async fn exists(&self, container: &str) -> Result<bool> {
        let out = self
            .run(
                &[
                    "inspect".to_owned(),
                    "-f".to_owned(),
                    "{{.State.Running}}".to_owned(),
                    container.to_owned(),
                ],
                Duration::from_secs(15),
                64,
            )
            .await?;
        Ok(out.status == Some(0) && out.stdout.trim() == "true")
    }
}

impl DockerTransport {
    /// Resolve the published loopback VNC port (`docker port`).
    async fn docker_port(&self, container: &str) -> Result<Option<u16>> {
        let out = self
            .run(
                &["port".to_owned(), container.to_owned(), "5900".to_owned()],
                Duration::from_secs(15),
                256,
            )
            .await?;
        if out.status != Some(0) {
            return Ok(None);
        }
        // `0.0.0.0:32768` or `127.0.0.1:32768` — take the trailing port.
        let port = out
            .stdout
            .trim()
            .rsplit(':')
            .next()
            .unwrap_or_default()
            .trim()
            .parse::<u16>()
            .ok();
        Ok(port)
    }
}

impl DockerTransport {
    /// Run one docker CLI call with a hard timeout and bounded output.
    async fn run(
        &self,
        argv: &[String],
        timeout: Duration,
        max_bytes: usize,
    ) -> Result<ExecOutput> {
        use tokio::io::AsyncReadExt as _;
        let (program, args) = (&self.program(), argv);
        let mut child = tokio::process::Command::new(program)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|error| err(format!("vm_unavailable: docker spawn failed: {error}")))?;
        let mut stdout_pipe = child.stdout.take();
        let mut stderr_pipe = child.stderr.take();
        let exit = tokio::select! {
            result = child.wait() => result.map_err(|error| err(format!("vm exec failed: {error}")))?,
            _ = tokio::time::sleep(timeout) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                return Err(err("vm exec timed out"));
            }
        };
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        if let Some(pipe) = stdout_pipe.as_mut() {
            let _ = pipe
                .take(max_bytes as u64 + 1)
                .read_to_end(&mut stdout)
                .await;
        }
        if let Some(pipe) = stderr_pipe.as_mut() {
            let _ = pipe.take(8193).read_to_end(&mut stderr).await;
        }
        let stdout_text = String::from_utf8_lossy(&stdout).into_owned();
        let mut stderr_text = String::from_utf8_lossy(&stderr).into_owned();
        if stderr_text.len() > 8192 {
            stderr_text.truncate(8192);
            stderr_text.push_str("…[truncated]");
        }
        Ok(ExecOutput {
            status: exit.code(),
            stdout: stdout_text,
            stderr: stderr_text,
        })
    }
}

/// Hermetic transport for tests and CI (no daemon, no network).
///
/// Simulates one running container per id: `import` returns a fixed valid
/// PNG, `xdotool` input is recorded, guest files live in memory, lifecycle
/// calls mutate a visible log for assertions.
#[derive(Debug, Default)]
pub struct MockTransport {
    inner: std::sync::Mutex<MockState>,
}

#[derive(Debug, Default)]
struct MockState {
    containers: HashMap<String, MockContainer>,
    log: Vec<String>,
    next: u64,
}

#[derive(Debug)]
struct MockContainer {
    input_log: Vec<Vec<String>>,
    files: HashMap<String, Vec<u8>>,
    paused: bool,
}

/// Fixed 8x8 RGBA PNG (base64) returned as every mock frame.
pub const MOCK_FRAME_PNG_BASE64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAgAAAAICAIAAABLbSncAAAAJUlEQVR4nGP8//8/AzGAiShVwcDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwBgB7CQwC5rYDxQAAAABJRU5ErkJggg==";

fn mock_png() -> Vec<u8> {
    decode_base64(MOCK_FRAME_PNG_BASE64)
}

/// Minimal base64 decoder (mock frames only; rejects garbage fail-closed).
fn decode_base64(input: &str) -> Vec<u8> {
    fn quad_value(byte: u8) -> Option<u8> {
        match byte {
            b'A'..=b'Z' => Some(byte - b'A'),
            b'a'..=b'z' => Some(byte - b'a' + 26),
            b'0'..=b'9' => Some(byte - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let bytes: Vec<u8> = input.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        if chunk.len() < 4 {
            break;
        }
        let mut quad = [0u8; 4];
        let mut pad = 0;
        for (i, byte) in chunk.iter().enumerate() {
            if *byte == b'=' {
                pad += 1;
            } else if let Some(value) = quad_value(*byte) {
                quad[i] = value;
            } else {
                return Vec::new();
            }
        }
        out.push((quad[0] << 2) | (quad[1] >> 4));
        if pad < 2 {
            out.push(((quad[1] & 0x0F) << 4) | (quad[2] >> 2));
        }
        if pad == 0 {
            out.push(((quad[2] & 0x03) << 6) | quad[3]);
        }
    }
    out
}

impl MockTransport {
    /// Operation log for assertions (`spawn <id>`, `exec <id> <argv…>`, …).
    pub fn take_log(&self) -> Vec<String> {
        std::mem::take(&mut self.inner.lock().expect("mock").log)
    }

    /// Recorded xdotool argv per container.
    pub fn input_log(&self, container: &str) -> Vec<Vec<String>> {
        self.inner
            .lock()
            .expect("mock")
            .containers
            .get(container)
            .map(|c| c.input_log.clone())
            .unwrap_or_default()
    }

    /// Seed a guest file for `copy_out` tests.
    pub fn seed_file(&self, container: &str, guest: &str, bytes: Vec<u8>) {
        if let Some(entry) = self
            .inner
            .lock()
            .expect("mock")
            .containers
            .get_mut(container)
        {
            entry.files.insert(guest.to_owned(), bytes);
        }
    }
}

#[async_trait::async_trait]
impl VmTransport for MockTransport {
    async fn spawn(&self, spec: &ContainerSpec) -> Result<ContainerInfo> {
        let mut state = self.inner.lock().expect("mock");
        state.next += 1;
        let id = format!("mock-{:04}", state.next);
        state.log.push(format!(
            "spawn {id} {} ro:{}",
            spec.name,
            spec.workspace_host.display()
        ));
        state.containers.insert(
            id.clone(),
            MockContainer {
                input_log: Vec::new(),
                files: HashMap::new(),
                paused: false,
            },
        );
        Ok(ContainerInfo {
            id,
            name: spec.name.clone(),
            vnc_port: spec.vnc_expose.then_some(15901),
        })
    }

    async fn exec(&self, container: &str, argv: &[String]) -> Result<ExecOutput> {
        let mut guard = self.inner.lock().expect("mock");
        let paused = guard
            .containers
            .get(container)
            .ok_or_else(|| err("vm_not_running: unknown container"))?
            .paused;
        if paused {
            return Err(err("vm exec failed: container paused"));
        }
        // Screenshot path serves the fixed frame; everything else is input.
        if !argv.first().is_some_and(|first| first == "import") {
            guard
                .containers
                .get_mut(container)
                .expect("present")
                .input_log
                .push(argv.to_vec());
        }
        guard
            .log
            .push(format!("exec {container} {}", argv.join(" ")));
        Ok(ExecOutput {
            status: Some(0),
            stdout: String::new(),
            stderr: String::new(),
        })
    }

    async fn copy_out(&self, container: &str, guest: &str, host: &Path) -> Result<u64> {
        let bytes = self
            .inner
            .lock()
            .expect("mock")
            .containers
            .get(container)
            .and_then(|entry| entry.files.get(guest).cloned())
            .ok_or_else(|| err("export_missing: guest artifact not found"))?;
        if bytes.len() > MAX_EXPORT_BYTES {
            return Err(err("export_rejected: artifact exceeds 5 MiB"));
        }
        if let Some(parent) = host.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| err(format!("vm export staging failed: {error}")))?;
        }
        std::fs::write(host, &bytes)
            .map_err(|error| err(format!("vm export write failed: {error}")))?;
        Ok(bytes.len() as u64)
    }

    async fn fetch_png(&self, container: &str) -> Result<Vec<u8>> {
        let state = self.inner.lock().expect("mock");
        if !state.containers.contains_key(container) {
            return Err(err("vm_not_running: unknown container"));
        }
        Ok(mock_png())
    }

    async fn pause(&self, container: &str) -> Result<()> {
        let mut state = self.inner.lock().expect("mock");
        let entry = state
            .containers
            .get_mut(container)
            .ok_or_else(|| err("vm_not_running: unknown container"))?;
        entry.paused = true;
        state.log.push(format!("pause {container}"));
        Ok(())
    }

    async fn unpause(&self, container: &str) -> Result<()> {
        let mut state = self.inner.lock().expect("mock");
        let entry = state
            .containers
            .get_mut(container)
            .ok_or_else(|| err("vm_not_running: unknown container"))?;
        entry.paused = false;
        state.log.push(format!("unpause {container}"));
        Ok(())
    }

    async fn remove(&self, container: &str) -> Result<()> {
        let mut state = self.inner.lock().expect("mock");
        state.containers.remove(container);
        state.log.push(format!("remove {container}"));
        Ok(())
    }

    async fn exists(&self, container: &str) -> Result<bool> {
        Ok(self
            .inner
            .lock()
            .expect("mock")
            .containers
            .contains_key(container))
    }
}

/// One held ephemeral session (exactly one per manager).
#[derive(Debug)]
struct ActiveVm {
    container: ContainerInfo,
    config: VmConfig,
    workspace_host: PathBuf,
    started_at: String,
    last_used_ms: u64,
    frame_seq: u64,
    task_id: String,
}

/// Lifecycle manager: spawn on demand, drive, export, destroy.
///
/// Exactly one active container per manager (the desktop shell and the
/// gateway daemon each own one; single-task focus keeps the blast radius
/// minimal). All guest I/O goes through the [`VmTransport`].
pub struct VmManager {
    transport: Arc<dyn VmTransport>,
    active: tokio::sync::Mutex<Option<ActiveVm>>,
    now_ms: Box<dyn Fn() -> u64 + Send + Sync>,
}

impl std::fmt::Debug for VmManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VmManager").finish_non_exhaustive()
    }
}

impl VmManager {
    /// Build around an explicit transport (production: [`DockerTransport`]).
    pub fn new(transport: Arc<dyn VmTransport>) -> Self {
        Self::new_with_clock(transport, Box::new(now_ms))
    }

    /// Build with an injectable clock (idle-sweep tests).
    pub fn new_with_clock(
        transport: Arc<dyn VmTransport>,
        now_ms: Box<dyn Fn() -> u64 + Send + Sync>,
    ) -> Self {
        Self {
            transport,
            active: tokio::sync::Mutex::new(None),
            now_ms,
        }
    }

    /// Process-global manager (production Docker transport).
    pub fn global() -> Arc<Self> {
        static GLOBAL: OnceLock<Arc<VmManager>> = OnceLock::new();
        GLOBAL
            .get_or_init(|| Arc::new(VmManager::new(Arc::new(DockerTransport::default()))))
            .clone()
    }

    fn now(&self) -> u64 {
        (self.now_ms)()
    }

    /// Status snapshot for UI panels and events.
    pub async fn status(&self) -> VmSessionView {
        match self.active.lock().await.as_ref() {
            None => VmSessionView {
                running: false,
                container_id: None,
                container_name: None,
                vnc_url: None,
                width: VmConfig::default().width,
                height: VmConfig::default().height,
                started_at: None,
                backend: "ephemeral-vm".to_owned(),
            },
            Some(active) => VmSessionView {
                running: true,
                container_id: Some(active.container.id.clone()),
                container_name: Some(active.container.name.clone()),
                vnc_url: active
                    .container
                    .vnc_port
                    .map(|port| format!("vnc://127.0.0.1:{port}")),
                width: active.config.width,
                height: active.config.height,
                started_at: Some(active.started_at.clone()),
                backend: "ephemeral-vm".to_owned(),
            },
        }
    }

    /// Ensure a running session: reuse the warm container when healthy,
    /// otherwise destroy the stale one and spawn fresh.
    ///
    /// The workspace is validated (`resolve_host_workspace`) and mounted
    /// read-only; the host root can never slip through.
    pub async fn ensure_running(
        &self,
        task_id: &str,
        workspace_host: &Path,
        config: &VmConfig,
    ) -> Result<VmSessionView> {
        config.validate()?;
        let workspace = resolve_host_workspace(workspace_host).await?;
        let mut active = self.active.lock().await;
        if let Some(current) = active.as_mut() {
            if current.config.image == config.image
                && current.workspace_host == workspace
                && self
                    .transport
                    .exists(&current.container.id)
                    .await
                    .unwrap_or(false)
            {
                current.last_used_ms = self.now();
                current.task_id = task_id.to_owned();
                let view = self.view_locked(current);
                return Ok(view);
            }
            let stale = current.container.id.clone();
            *active = None;
            drop(active);
            let _ = self.transport.remove(&stale).await;
            active = self.active.lock().await;
        }
        let name = format!(
            "{}-{}-{}",
            config.container_name_prefix,
            sanitize_name(task_id),
            self.now() % 100_000
        );
        let spec = ContainerSpec {
            name: name.clone(),
            image: config.image.clone(),
            workspace_host: workspace.clone(),
            width: config.width,
            height: config.height,
            memory_mb: config.memory_mb,
            cpus: config.cpus.clone(),
            vnc_expose: config.vnc_expose,
        };
        let container = self.transport.spawn(&spec).await?;
        let view = VmSessionView {
            running: true,
            container_id: Some(container.id.clone()),
            container_name: Some(container.name.clone()),
            vnc_url: container
                .vnc_port
                .map(|port| format!("vnc://127.0.0.1:{port}")),
            width: config.width,
            height: config.height,
            started_at: Some(talos_common::now_iso()),
            backend: "ephemeral-vm".to_owned(),
        };
        *active = Some(ActiveVm {
            container,
            config: config.clone(),
            workspace_host: workspace,
            started_at: talos_common::now_iso(),
            last_used_ms: self.now(),
            frame_seq: 0,
            task_id: task_id.to_owned(),
        });
        Ok(view)
    }

    /// Capture the current guest frame as a [`VmFrame`].
    pub async fn frame(&self, with_png: bool) -> Result<VmFrame> {
        let mut active = self.active.lock().await;
        let current = active.as_mut().ok_or_else(|| err("vm_not_running"))?;
        let png = self.transport.fetch_png(&current.container.id).await?;
        if png.len() > MAX_FRAME_BYTES {
            return Err(err("vm_frame_too_large: frame exceeds 2 MiB"));
        }
        if !is_png(&png) {
            return Err(err("vm frame is not a PNG"));
        }
        current.last_used_ms = self.now();
        current.frame_seq += 1;
        let hash = crate::vision::hash_bytes(&png);
        Ok(VmFrame {
            seq: current.frame_seq,
            hash,
            width: current.config.width,
            height: current.config.height,
            png_base64: with_png.then(|| crate::vision::base64_encode(&png)),
            at: talos_common::now_iso(),
        })
    }

    /// Drive one resolved action inside the guest (input + fresh frame hash).
    pub async fn act(&self, action: &DriverAction) -> Result<(VmFrame, ExecOutput)> {
        let argv = xdotool_argv(action)?;
        let (container, width, height) = {
            let active = self.active.lock().await;
            let current = active.as_ref().ok_or_else(|| err("vm_not_running"))?;
            (
                current.container.id.clone(),
                current.config.width,
                current.config.height,
            )
        };
        let full: Vec<String> = argv;
        // `exec` runs `xdotool <argv…>`; the mock records it as input.
        let out = self
            .transport
            .exec(&container, &full)
            .await
            .map_err(|_| err("vm exec failed"))?;
        if out.status != Some(0) {
            return Err(err("vm exec failed"));
        }
        let frame = self.frame(false).await?;
        let _ = (width, height);
        Ok((frame, out))
    }

    /// Copy a verified artifact out of the guest export dir.
    ///
    /// `guest_rel` is confined to [`GUEST_EXPORT_DIR`]; `host_dest` must sit
    /// inside the caller-provided task root. The destination parent is
    /// created, then canonicalized (resolving Windows `\\?\` prefixes and
    /// symlinks) before the containment check, so the check cannot be
    /// fooled by path spelling.
    pub async fn export_file(
        &self,
        guest_rel: &str,
        host_dest: &Path,
        task_root: &Path,
    ) -> Result<(u64, String)> {
        let guest = resolve_export_guest(guest_rel)?;
        let canonical_root = tokio::fs::canonicalize(task_root)
            .await
            .map_err(|error| err(format!("vm export root unresolvable: {error}")))?;
        let raw_dest = if host_dest.is_absolute() {
            host_dest.to_path_buf()
        } else {
            canonical_root.join(host_dest)
        };
        let file_name = raw_dest
            .file_name()
            .ok_or_else(|| err("export_rejected: host destination has no file name"))?;
        let raw_parent = raw_dest
            .parent()
            .ok_or_else(|| err("export_rejected: host destination escapes the task root"))?;
        // Create then canonicalize the parent (resolves `\\?\` prefixes and
        // symlinks) before the containment check — spelling can't fool it.
        tokio::fs::create_dir_all(raw_parent)
            .await
            .map_err(|error| err(format!("vm export staging failed: {error}")))?;
        let canonical_parent = tokio::fs::canonicalize(raw_parent)
            .await
            .map_err(|error| err(format!("vm export staging failed: {error}")))?;
        if !canonical_parent.starts_with(&canonical_root) {
            return Err(err(
                "export_rejected: host destination escapes the task root",
            ));
        }
        let dest = canonical_parent.join(file_name);
        let container = {
            let active = self.active.lock().await;
            active
                .as_ref()
                .ok_or_else(|| err("vm_not_running"))?
                .container
                .id
                .clone()
        };
        let bytes = self.transport.copy_out(&container, &guest, &dest).await?;
        let data = tokio::fs::read(&dest)
            .await
            .map_err(|error| err(format!("vm export readback failed: {error}")))?;
        let hash = crate::vision::hash_bytes(&data);
        {
            let mut active = self.active.lock().await;
            if let Some(current) = active.as_mut() {
                current.last_used_ms = self.now();
            }
        }
        Ok((bytes, hash))
    }

    /// Destroy the active container (idempotent).
    pub async fn destroy(&self) -> Result<bool> {
        let id = {
            let mut active = self.active.lock().await;
            active.take().map(|current| current.container.id)
        };
        match id {
            None => Ok(false),
            Some(id) => {
                self.transport.remove(&id).await?;
                Ok(true)
            }
        }
    }

    /// Freeze all guest processes (emergency-stop path).
    pub async fn pause_all(&self) -> Result<bool> {
        let id = {
            let active = self.active.lock().await;
            active.as_ref().map(|current| current.container.id.clone())
        };
        match id {
            None => Ok(false),
            Some(id) => {
                self.transport.pause(&id).await?;
                Ok(true)
            }
        }
    }

    /// Unfreeze guest processes (explicit reset path).
    pub async fn resume_all(&self) -> Result<bool> {
        let id = {
            let active = self.active.lock().await;
            active.as_ref().map(|current| current.container.id.clone())
        };
        match id {
            None => Ok(false),
            Some(id) => {
                self.transport.unpause(&id).await?;
                Ok(true)
            }
        }
    }

    /// Destroy sessions idle longer than the configured timeout.
    pub async fn sweep_idle(&self) -> Result<bool> {
        let (id, idle_ms, timeout) = {
            let active = self.active.lock().await;
            match active.as_ref() {
                None => return Ok(false),
                Some(current) => (
                    current.container.id.clone(),
                    self.now().saturating_sub(current.last_used_ms),
                    current.config.idle_timeout_secs,
                ),
            }
        };
        if timeout == 0 || idle_ms < timeout * 1000 {
            return Ok(false);
        }
        {
            let mut active = self.active.lock().await;
            active.take();
        }
        let _ = self.transport.remove(&id).await;
        Ok(true)
    }

    /// Touch the session clock (receipt paths call this after acting).
    pub async fn touch(&self) {
        if let Some(current) = self.active.lock().await.as_mut() {
            current.last_used_ms = self.now();
        }
    }

    fn view_locked(&self, current: &ActiveVm) -> VmSessionView {
        VmSessionView {
            running: true,
            container_id: Some(current.container.id.clone()),
            container_name: Some(current.container.name.clone()),
            vnc_url: current
                .container
                .vnc_port
                .map(|port| format!("vnc://127.0.0.1:{port}")),
            width: current.config.width,
            height: current.config.height,
            started_at: Some(current.started_at.clone()),
            backend: "ephemeral-vm".to_owned(),
        }
    }
}

/// Sanitize a task id into a container-name fragment.
fn sanitize_name(task_id: &str) -> String {
    task_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .chars()
        .take(32)
        .collect()
}

/// Millisecond clock for idle accounting.
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::DriverAction;

    fn mock_manager() -> (VmManager, Arc<MockTransport>) {
        let transport = Arc::new(MockTransport::default());
        let manager = VmManager::new(transport.clone() as Arc<dyn VmTransport>);
        (manager, transport)
    }

    fn workspace() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    #[test]
    fn config_validates_bounds() {
        assert!(VmConfig::default().validate().is_ok());
        assert!(VmConfig {
            width: 100,
            ..VmConfig::default()
        }
        .validate()
        .is_err());
        assert!(VmConfig {
            memory_mb: 64,
            ..VmConfig::default()
        }
        .validate()
        .is_err());
        assert!(VmConfig {
            idle_timeout_secs: 9999,
            ..VmConfig::default()
        }
        .validate()
        .is_err());
    }

    #[test]
    fn input_translation_maps_actions_to_xdotool() {
        assert_eq!(
            xdotool_argv(&DriverAction::MoveTo { x: 10, y: 20 }).expect("move"),
            vec!["mousemove", "10", "20"]
        );
        assert_eq!(
            xdotool_argv(&DriverAction::Click {
                x: 10,
                y: 20,
                button: "right".to_owned()
            })
            .expect("click"),
            vec!["mousemove", "10", "20", "click", "3"]
        );
        assert_eq!(
            xdotool_argv(&DriverAction::TypeText {
                text: "hi".to_owned()
            })
            .expect("type"),
            vec!["type", "--", "hi"]
        );
        assert_eq!(
            xdotool_argv(&DriverAction::Hotkey {
                keys: "ctrl+s".to_owned()
            })
            .expect("hotkey"),
            vec!["key", "ctrl+s"]
        );
        assert_eq!(
            xdotool_argv(&DriverAction::Scroll {
                x: None,
                y: None,
                direction: "down".to_owned(),
                pixels: 240
            })
            .expect("scroll"),
            vec!["click", "--repeat", "2", "5"]
        );
        assert!(xdotool_argv(&DriverAction::Click {
            x: 1,
            y: 2,
            button: "hyper".to_owned()
        })
        .is_err());
        assert!(xdotool_argv(&DriverAction::Activate).is_err());
    }

    #[test]
    fn export_paths_stay_inside_the_output_dir() {
        assert_eq!(
            resolve_export_guest("report.md").expect("rel"),
            "/home/agent/output/report.md"
        );
        assert_eq!(
            resolve_export_guest("a/b/c.png").expect("nested"),
            "/home/agent/output/a/b/c.png"
        );
        assert!(resolve_export_guest("/etc/passwd").is_err());
        assert!(resolve_export_guest("../escape").is_err());
        assert!(resolve_export_guest("a/../../escape").is_err());
        assert!(resolve_export_guest("").is_err());
    }

    #[tokio::test]
    async fn host_root_is_never_a_workspace() {
        assert!(resolve_host_workspace(Path::new("/")).await.is_err());
        assert!(resolve_host_workspace(Path::new("/nonexistent-talos-vm"))
            .await
            .is_err());
        let dir = workspace();
        let file = dir.path().join("f.txt");
        tokio::fs::write(&file, b"x").await.expect("write");
        assert!(resolve_host_workspace(&file).await.is_err());
        assert!(resolve_host_workspace(dir.path()).await.is_ok());
    }

    /// Container lifecycle: spin up, execute a mock click, tear down.
    #[tokio::test]
    async fn lifecycle_spin_up_click_tear_down() {
        let (manager, transport) = mock_manager();
        let dir = workspace();
        assert!(!manager.status().await.running);
        let view = manager
            .ensure_running("task-1", dir.path(), &VmConfig::default())
            .await
            .expect("spawn");
        assert!(view.running);
        assert!(view.container_id.is_some());
        assert_eq!(view.backend, "ephemeral-vm");
        assert!(view.vnc_url.is_none(), "vnc off by default");
        // Second ensure reuses the warm container (no second spawn).
        let again = manager
            .ensure_running("task-1", dir.path(), &VmConfig::default())
            .await
            .expect("reuse");
        assert_eq!(again.container_id, view.container_id);
        // Execute a mock click: input is recorded, frame seq advances.
        let (frame, _) = manager
            .act(&DriverAction::Click {
                x: 100,
                y: 200,
                button: "left".to_owned(),
            })
            .await
            .expect("act");
        assert_eq!(frame.seq, 1);
        assert!(frame.hash.starts_with("sha256:"));
        let id = view.container_id.clone().expect("id");
        let inputs = transport.input_log(&id);
        assert_eq!(inputs.len(), 1);
        assert_eq!(
            inputs[0],
            vec![
                "mousemove".to_owned(),
                "100".to_owned(),
                "200".to_owned(),
                "click".to_owned(),
                "1".to_owned()
            ]
        );
        // Frames stream with metadata + PNG.
        let streamed = manager.frame(true).await.expect("frame");
        assert_eq!(streamed.seq, 2);
        assert!(streamed.png_base64.is_some());
        // Tear down: destroy is idempotent.
        assert!(manager.destroy().await.expect("destroy"));
        assert!(!manager.status().await.running);
        assert!(!manager.destroy().await.expect("destroy again"));
        let log = transport.take_log();
        assert!(log.iter().any(|line| line.starts_with("spawn ")));
        assert!(log.iter().any(|line| line.starts_with("remove ")));
    }

    /// Filesystem isolation: the workspace mount is read-only and the host
    /// root is unreachable; the agent cannot see host files.
    #[tokio::test]
    async fn filesystem_isolation_holds() {
        let (manager, transport) = mock_manager();
        let dir = workspace();
        // A host marker the guest must never observe.
        tokio::fs::write(dir.path().join("host-only.txt"), b"host")
            .await
            .expect("marker");
        let view = manager
            .ensure_running("task-iso", dir.path(), &VmConfig::default())
            .await
            .expect("spawn");
        let log = transport.take_log();
        let spawn = log
            .iter()
            .find(|line| line.starts_with("spawn "))
            .expect("spawn logged");
        assert!(
            spawn.contains("ro:"),
            "workspace mount must be read-only: {spawn}"
        );
        // The mounted source is the resolved task workspace, never the root.
        let mount = spawn.rsplit("ro:").next().unwrap_or_default();
        assert!(mount.len() > 3, "mount source must be a real dir: {spawn}");
        assert!(
            mount != "/" && mount != "\\" && !mount.ends_with(":\\"),
            "host root must never mount: {spawn}"
        );
        // The mock guest starts empty: no host files leak in.
        let id = view.container_id.expect("id");
        assert!(transport.input_log(&id).is_empty());
        // Export only serves the agent output dir, never host paths.
        assert!(manager
            .export_file("/etc/passwd", Path::new("out.txt"), dir.path())
            .await
            .is_err());
        assert!(manager
            .export_file("../escape.txt", Path::new("out.txt"), dir.path())
            .await
            .is_err());
        // Host destinations outside the task root are refused (sibling of
        // the task dir, so containment cannot hold).
        let outside = std::env::temp_dir().join("talos-vm-outside.txt");
        assert!(manager
            .export_file("ok.txt", &outside, dir.path())
            .await
            .is_err());
        manager.destroy().await.expect("destroy");
    }

    /// Verified artifacts leave the VM through `export_file` with a hash.
    #[tokio::test]
    async fn export_file_round_trip() {
        let (manager, transport) = mock_manager();
        let dir = workspace();
        let view = manager
            .ensure_running("task-exp", dir.path(), &VmConfig::default())
            .await
            .expect("spawn");
        let id = view.container_id.expect("id");
        transport.seed_file(&id, "/home/agent/output/report.md", b"# verified".to_vec());
        let dest = dir.path().join("vm-exports").join("report.md");
        let (bytes, hash) = manager
            .export_file("report.md", &dest, dir.path())
            .await
            .expect("export");
        assert_eq!(bytes, 10);
        assert!(hash.starts_with("sha256:"));
        assert_eq!(tokio::fs::read(&dest).await.expect("read"), b"# verified");
        // Missing guest files fail closed (never an empty success).
        assert!(manager
            .export_file("missing.md", &dest, dir.path())
            .await
            .is_err());
        manager.destroy().await.expect("destroy");
    }

    #[tokio::test]
    async fn pause_resume_and_idle_sweep() {
        let clock = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(1_000));
        let tick = clock.clone();
        let transport: Arc<dyn VmTransport> = Arc::new(MockTransport::default());
        let manager = VmManager::new_with_clock(
            transport,
            Box::new(move || tick.load(std::sync::atomic::Ordering::SeqCst)),
        );
        let dir = workspace();
        manager
            .ensure_running(
                "task-idle",
                dir.path(),
                &VmConfig {
                    idle_timeout_secs: 60,
                    ..VmConfig::default()
                },
            )
            .await
            .expect("spawn");
        assert!(manager.pause_all().await.expect("pause"));
        assert!(manager.resume_all().await.expect("resume"));
        // Still warm inside the budget.
        assert!(!manager.sweep_idle().await.expect("sweep"));
        assert!(manager.status().await.running);
        // Past the budget the container is destroyed.
        clock.store(1_000 + 61_000, std::sync::atomic::Ordering::SeqCst);
        assert!(manager.sweep_idle().await.expect("sweep idle"));
        assert!(!manager.status().await.running);
        manager.destroy().await.expect("destroy");
    }

    #[tokio::test]
    async fn vnc_expose_reports_loopback_url() {
        let (manager, _) = mock_manager();
        let dir = workspace();
        let view = manager
            .ensure_running(
                "task-vnc",
                dir.path(),
                &VmConfig {
                    vnc_expose: true,
                    ..VmConfig::default()
                },
            )
            .await
            .expect("spawn");
        let url = view.vnc_url.expect("vnc url");
        assert!(url.starts_with("vnc://127.0.0.1:"), "loopback only: {url}");
        manager.destroy().await.expect("destroy");
    }
}
