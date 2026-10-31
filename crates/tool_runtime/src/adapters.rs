//! Execution adapters: dry-run planning, confined local execution, and a
//! future sandbox placeholder.
//!
//! Rules: dry-run is the default for risky tools; nothing here bypasses the
//! policy broker (gating happens in [`crate::executor` before any adapter
//! runs); all file access stays under a sandbox root with symlink-escape
//! rejection.

use std::path::{Component, Path, PathBuf};

use talos_common::{Result, TalosError};

/// Execution backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExecutionMode {
    /// Plan only (default for risky tools).
    #[default]
    DryRun,
    /// Confined local execution after policy approval.
    Live,
    /// Future isolated sandbox (currently unavailable by design).
    Sandbox,
}

/// Sandbox configuration owned by the host (desktop layer).
#[derive(Debug, Clone)]
pub struct SandboxConfig {
    /// Filesystem root every tool path resolves under.
    pub root: PathBuf,
    /// Live `process.exec` master switch (default false).
    pub exec_enabled: bool,
    /// Exact executable names allowed when `exec_enabled`.
    pub exec_allowlist: Vec<String>,
    /// Default exec timeout in milliseconds.
    pub exec_timeout_ms: u64,
    /// Largest file the tools will read/write in bytes.
    pub max_bytes: usize,
    /// Computer-use safety envelope (global enable flag, emergency stop,
    /// rate limit, bounds, dangerous zones). All computer tools are gated
    /// through it before any adapter runs.
    pub computer: computer_use::SafetyEnvelope,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            root: std::env::temp_dir().join("talos-tools"),
            exec_enabled: false,
            exec_allowlist: Vec::new(),
            exec_timeout_ms: 5_000,
            max_bytes: 262_144,
            computer: computer_use::SafetyEnvelope::default(),
        }
    }
}

/// Placeholder for the future isolated sandbox.
#[derive(Debug, Clone, Default)]
pub struct SandboxAdapter;

impl SandboxAdapter {
    /// Always unavailable: honesty over a fake sandbox.
    pub fn unavailable<T>(tool_id: &str) -> Result<T> {
        Err(TalosError::Storage(format!(
            "{tool_id}: sandbox adapter unavailable (local-safe execution only)"
        )))
    }
}

/// Reject absolute paths, `..` escapes, and empty input early.
pub fn check_relative(path: &str) -> Result<()> {
    if path.trim().is_empty() || path.len() > 1024 {
        return Err(TalosError::Storage(
            "path must be 1..=1024 chars".to_owned(),
        ));
    }
    let parsed = Path::new(path);
    if parsed.is_absolute() {
        return Err(TalosError::Storage(
            "absolute paths are rejected (sandbox-relative only)".to_owned(),
        ));
    }
    for component in parsed.components() {
        match component {
            Component::ParentDir => {
                return Err(TalosError::Storage(
                    "parent-dir (`..`) escapes are rejected".to_owned(),
                ));
            }
            Component::Prefix(_) | Component::RootDir => {
                return Err(TalosError::Storage(
                    "absolute paths are rejected".to_owned(),
                ));
            }
            Component::CurDir | Component::Normal(_) => {}
        }
    }
    Ok(())
}

/// Join a sandbox-relative path under `root` (lexical checks first).
pub fn join_under_root(root: &Path, path: &str) -> Result<PathBuf> {
    check_relative(path)?;
    Ok(root.join(path))
}

/// Canonicalize and verify containment (rejects symlink escapes).
///
/// For missing files (e.g. `fs.write` targets), the parent must exist (or be
/// created by the caller) and the canonical parent must stay under the
/// canonical root.
pub async fn contained_path(root: &Path, joined: &Path) -> Result<PathBuf> {
    let canonical_root = tokio::fs::canonicalize(root)
        .await
        .map_err(|err| TalosError::Storage(format!("sandbox root unavailable: {err}")))?;
    if let Ok(canonical) = tokio::fs::canonicalize(joined).await {
        if !canonical.starts_with(&canonical_root) {
            return Err(TalosError::Storage(
                "symlink escape rejected (outside sandbox root)".to_owned(),
            ));
        }
        return Ok(canonical);
    }
    // Missing target: verify the nearest existing ancestor stays inside.
    let mut ancestor = joined;
    while ancestor.parent().is_some() && tokio::fs::metadata(ancestor).await.is_err() {
        ancestor = ancestor.parent().unwrap_or(Path::new(""));
    }
    if ancestor.as_os_str().is_empty() {
        return Err(TalosError::Storage("unresolvable path".to_owned()));
    }
    let canonical_ancestor = tokio::fs::canonicalize(ancestor)
        .await
        .map_err(|err| TalosError::Storage(format!("ancestor unavailable: {err}")))?;
    if !canonical_ancestor.starts_with(&canonical_root) {
        return Err(TalosError::Storage(
            "path escapes the sandbox root".to_owned(),
        ));
    }
    Ok(joined.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_escapes_lexically() {
        assert!(check_relative("../etc/passwd").is_err());
        assert!(check_relative("/abs/path").is_err());
        assert!(check_relative("ok/relative.txt").is_ok());
    }

    #[tokio::test]
    async fn rejects_symlink_escape() {
        #[cfg(unix)]
        {
            let root = tempfile::tempdir().expect("tempdir");
            let outside = tempfile::tempdir().expect("tempdir");
            tokio::fs::write(outside.path().join("secret.txt"), b"secret")
                .await
                .expect("write");
            std::os::unix::fs::symlink(outside.path(), root.path().join("link")).expect("symlink");
            let joined = root.path().join("link/secret.txt");
            assert!(contained_path(root.path(), &joined).await.is_err());
        }
    }
}
