//! Content-addressed screenshot artifacts.
//!
//! Real captures are persisted under `<root>/computer-artifacts/` keyed by
//! their `sha256:` hash (`sha256-<hex>.png`). Identical frames deduplicate
//! naturally; evidence rows reference the hash, never raw pixels.

use std::path::{Path, PathBuf};

use talos_common::{Result, TalosError};

use crate::vision::hash_bytes;

/// Stored artifact reference: hash + filesystem path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredArtifact {
    /// `sha256:` hex of the PNG bytes.
    pub hash: String,
    /// Absolute path of the stored file.
    pub path: PathBuf,
}

fn err(msg: &str) -> TalosError {
    TalosError::Storage(msg.to_owned())
}

/// Artifact directory for one sandbox root.
#[must_use]
pub fn artifact_dir(root: &Path) -> PathBuf {
    root.join("computer-artifacts")
}

/// File name for a `sha256:<hex>` hash.
#[must_use]
pub fn artifact_file_name(hash: &str) -> String {
    let hex = hash.strip_prefix("sha256:").unwrap_or(hash);
    format!("sha256-{hex}.png")
}

/// Store PNG bytes content-addressed; returns the hash + path.
///
/// Writes are idempotent: an existing hash is never rewritten.
pub fn store_screenshot(root: &Path, png: &[u8]) -> Result<StoredArtifact> {
    if png.is_empty() {
        return Err(err("cannot store an empty screenshot"));
    }
    let hash = hash_bytes(png);
    let dir = artifact_dir(root);
    std::fs::create_dir_all(&dir).map_err(|error| err(&format!("artifact dir: {error}")))?;
    let path = dir.join(artifact_file_name(&hash));
    if !path.exists() {
        std::fs::write(&path, png).map_err(|error| err(&format!("artifact write: {error}")))?;
    }
    Ok(StoredArtifact { hash, path })
}

/// Load a stored artifact by hash.
pub fn load_screenshot(root: &Path, hash: &str) -> Result<Vec<u8>> {
    let path = artifact_dir(root).join(artifact_file_name(hash));
    std::fs::read(&path).map_err(|error| err(&format!("artifact read: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_is_content_addressed_and_idempotent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let first = store_screenshot(dir.path(), b"png-bytes").expect("stores");
        let second = store_screenshot(dir.path(), b"png-bytes").expect("stores again");
        assert_eq!(first, second);
        assert!(first.path.exists());
        let other = store_screenshot(dir.path(), b"other-bytes").expect("stores");
        assert_ne!(first.hash, other.hash);
        let loaded = load_screenshot(dir.path(), &first.hash).expect("loads");
        assert_eq!(loaded, b"png-bytes");
    }

    #[test]
    fn empty_screenshots_are_rejected() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(store_screenshot(dir.path(), b"").is_err());
    }
}
