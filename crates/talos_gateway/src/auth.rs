//! Per-start daemon credentials: token + PID files under the app-data dir.
//!
//! The auth token is generated fresh on every boot (`GatewayState` owns it),
//! written owner-only (0600 on Unix; user profile dir on Windows), and read
//! back by local clients (the Tauri shell). It is never logged, never
//! accepted from URL query strings, and never leaves `127.0.0.1`.

use std::path::Path;

use talos_common::{Result, TalosError};

use crate::protocol::{GATEWAY_PID_FILE_NAME, GATEWAY_TOKEN_FILE_NAME};

/// Write `token` to `<dir>/gateway.token`, owner-only. Called once, after a
/// successful bind, so a second instance that fails to bind can never
/// clobber the running daemon's token.
pub fn write_token_file(dir: &Path, token: &str) -> Result<()> {
    write_owner_only(&dir.join(GATEWAY_TOKEN_FILE_NAME), token)
}

/// Read the current daemon token (written at daemon boot).
pub fn read_token_file(dir: &Path) -> Result<String> {
    let text = std::fs::read_to_string(dir.join(GATEWAY_TOKEN_FILE_NAME))
        .map_err(|err| TalosError::Storage(format!("read gateway token: {err}")))?;
    let token = text.trim().to_owned();
    if token.is_empty() {
        return Err(TalosError::Storage(
            "gateway token file is empty".to_owned(),
        ));
    }
    Ok(token)
}

/// Write the daemon PID for best-effort restart coordination.
pub fn write_pid_file(dir: &Path) -> Result<()> {
    write_owner_only(
        &dir.join(GATEWAY_PID_FILE_NAME),
        &std::process::id().to_string(),
    )
}

/// Read a previously written daemon PID, if any.
pub fn read_pid_file(dir: &Path) -> Option<u32> {
    std::fs::read_to_string(dir.join(GATEWAY_PID_FILE_NAME))
        .ok()
        .and_then(|text| text.trim().parse().ok())
}

fn write_owner_only(path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| {
            TalosError::Storage(format!("create {} dir: {err}", parent.display()))
        })?;
    }
    std::fs::write(path, contents)
        .map_err(|err| TalosError::Storage(format!("write {}: {err}", path.display())))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|err| TalosError::Storage(format!("chmod 0600 {}: {err}", path.display())))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> tempfile::TempDir {
        tempfile::tempdir().expect("temp dir")
    }

    #[test]
    fn token_round_trips_and_never_empty() {
        let dir = dir();
        write_token_file(dir.path(), "tok-abc").expect("write");
        assert_eq!(read_token_file(dir.path()).expect("read"), "tok-abc");
        assert!(read_token_file(&dir.path().join("missing")).is_err());
    }

    #[test]
    fn pid_round_trips() {
        let dir = dir();
        assert!(read_pid_file(dir.path()).is_none());
        write_pid_file(dir.path()).expect("write");
        assert_eq!(
            read_pid_file(dir.path()),
            Some(std::process::id()),
            "own pid recorded"
        );
    }

    #[cfg(unix)]
    #[test]
    fn token_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = dir();
        write_token_file(dir.path(), "tok-abc").expect("write");
        let mode = std::fs::metadata(dir.path().join(GATEWAY_TOKEN_FILE_NAME))
            .expect("stat")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "owner read/write only");
    }
}
