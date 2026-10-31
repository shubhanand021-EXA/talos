//! Safe built-in filesystem tools (confined, bounded, hashed).
//!
//! All paths resolve under the sandbox root with symlink-escape rejection.
//! Reads are capped (`max_bytes`); listings cap depth and entries; searches
//! cap files and matches and skip non-UTF-8 content; writes cap content and
//! always report backup metadata for compensation.

use sha2::{Digest, Sha256};
use talos_common::{Result, TalosError};

use crate::adapters::{contained_path, join_under_root, SandboxConfig};

fn err(msg: &str) -> TalosError {
    TalosError::Storage(msg.to_owned())
}

fn hash_bytes(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

/// Read a file: metadata + hash + UTF-8 text when decodable.
pub async fn fs_read(config: &SandboxConfig, path: &str) -> Result<serde_json::Value> {
    let joined = join_under_root(&config.root, path)?;
    let full = contained_path(&config.root, &joined).await?;
    let metadata = tokio::fs::metadata(&full)
        .await
        .map_err(|e| err(&format!("fs.read: {e}")))?;
    if !metadata.is_file() {
        return Err(err("fs.read: not a file"));
    }
    if metadata.len() > config.max_bytes as u64 {
        return Err(err("fs.read: file exceeds max_bytes"));
    }
    let bytes = tokio::fs::read(&full)
        .await
        .map_err(|e| err(&format!("fs.read: {e}")))?;
    let truncated = bytes.len() > config.max_bytes;
    let bytes = if truncated {
        bytes[..config.max_bytes].to_vec()
    } else {
        bytes
    };
    let hash = hash_bytes(&bytes);
    let text = String::from_utf8(bytes.clone()).ok();
    Ok(serde_json::json!({
        "path": path,
        "bytes": bytes.len(),
        "hash": hash,
        "truncated": truncated,
        "text": text,
    }))
}

/// Directory entry row.
fn entry_row(name: String, kind: &str) -> serde_json::Value {
    serde_json::json!({ "name": name, "kind": kind })
}

/// List a directory (bounded depth and entries).
pub async fn fs_list(config: &SandboxConfig, path: &str, depth: u8) -> Result<serde_json::Value> {
    let scope = if path.trim().is_empty() { "." } else { path };
    let joined = join_under_root(&config.root, scope)?;
    let full = contained_path(&config.root, &joined).await?;
    let metadata = tokio::fs::metadata(&full)
        .await
        .map_err(|e| err(&format!("fs.list: {e}")))?;
    if !metadata.is_dir() {
        return Err(err("fs.list: not a directory"));
    }
    let mut entries = Vec::new();
    let mut truncated = false;
    let mut stack: Vec<(std::path::PathBuf, u8)> = vec![(full, 0)];
    while let Some((dir, level)) = stack.pop() {
        let mut stream = tokio::fs::read_dir(&dir)
            .await
            .map_err(|e| err(&format!("fs.list: {e}")))?;
        while let Some(child) = stream
            .next_entry()
            .await
            .map_err(|e| err(&format!("fs.list: {e}")))?
        {
            if entries.len() >= 200 {
                truncated = true;
                break;
            }
            let name = child.file_name().to_string_lossy().into_owned();
            let file_type = child
                .file_type()
                .await
                .map_err(|e| err(&format!("fs.list: {e}")))?;
            if file_type.is_symlink() {
                // Resolve and verify containment; skip escapes silently.
                let target = tokio::fs::canonicalize(child.path()).await;
                let root = tokio::fs::canonicalize(&config.root).await;
                match (target, root) {
                    (Ok(target), Ok(root)) if target.starts_with(&root) => {
                        let kind = if target.is_dir() { "dir" } else { "file" };
                        entries.push(entry_row(name.clone(), kind));
                        if kind == "dir" && level < depth {
                            stack.push((target, level + 1));
                        }
                    }
                    _ => {}
                }
                continue;
            }
            if file_type.is_dir() {
                entries.push(entry_row(name, "dir"));
                if level < depth {
                    stack.push((child.path(), level + 1));
                }
            } else {
                entries.push(entry_row(name, "file"));
            }
        }
        if truncated {
            break;
        }
    }
    entries.sort_by(|a, b| {
        a.get("name")
            .and_then(|value| value.as_str())
            .cmp(&b.get("name").and_then(|value| value.as_str()))
    });
    Ok(serde_json::json!({ "path": scope, "entries": entries, "truncated": truncated }))
}

/// Search UTF-8 text under a scope (bounded files and matches).
pub async fn fs_search(
    config: &SandboxConfig,
    query: &str,
    scope: &str,
    max_matches: usize,
) -> Result<serde_json::Value> {
    let scope = if scope.trim().is_empty() { "." } else { scope };
    let joined = join_under_root(&config.root, scope)?;
    let full = contained_path(&config.root, &joined).await?;
    let mut matches = Vec::new();
    let mut truncated = false;
    let mut files_seen = 0_usize;
    let mut stack = vec![full];
    while let Some(dir) = stack.pop() {
        let mut stream = match tokio::fs::read_dir(&dir).await {
            Ok(stream) => stream,
            Err(_) => continue,
        };
        while let Some(child) = stream
            .next_entry()
            .await
            .map_err(|e| TalosError::Storage(format!("fs.search: {e}")))?
        {
            if matches.len() >= max_matches || files_seen >= 200 {
                truncated = true;
                break;
            }
            let file_type = child
                .file_type()
                .await
                .map_err(|e| TalosError::Storage(format!("fs.search: {e}")))?;
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                stack.push(child.path());
                continue;
            }
            files_seen += 1;
            let bytes = match tokio::fs::read(child.path()).await {
                Ok(bytes) => bytes,
                Err(_) => continue,
            };
            if bytes.len() > config.max_bytes {
                continue;
            }
            let text = match String::from_utf8(bytes) {
                Ok(text) => text,
                Err(_) => continue,
            };
            for (index, line) in text.lines().enumerate() {
                if matches.len() >= max_matches {
                    truncated = true;
                    break;
                }
                if line.contains(query) {
                    let path = child
                        .path()
                        .strip_prefix(&config.root)
                        .map(|relative| relative.to_string_lossy().into_owned())
                        .unwrap_or_else(|_| child.file_name().to_string_lossy().into_owned());
                    matches.push(serde_json::json!({
                        "path": path,
                        "line": index + 1,
                        "preview": line.chars().take(160).collect::<String>(),
                    }));
                }
            }
        }
        if truncated {
            break;
        }
    }
    Ok(serde_json::json!({ "query": query, "matches": matches, "truncated": truncated }))
}

/// Planned write effects (dry-run: no mutation).
#[must_use]
pub fn fs_write_plan(path: &str, content: &str, existed: bool) -> serde_json::Value {
    serde_json::json!({
        "path": path,
        "bytes": content.len(),
        "overwrote": existed,
        "backup": { "existed": existed },
        "planned": true,
    })
}

/// Write a file (live): parents optional, backup metadata always reported.
pub async fn fs_write(
    config: &SandboxConfig,
    path: &str,
    content: &str,
    create_parents: bool,
) -> Result<serde_json::Value> {
    if content.len() > config.max_bytes {
        return Err(err("fs.write: content exceeds max_bytes"));
    }
    let joined = join_under_root(&config.root, path)?;
    // Ensure parents exist (or fail loudly when not allowed).
    if let Some(parent) = joined.parent() {
        let parent_exists = tokio::fs::metadata(parent).await.is_ok();
        if !parent_exists {
            if !create_parents {
                return Err(err("fs.write: parent directory missing"));
            }
            // Verify the parent chain stays inside before creating.
            let mut ancestor = parent;
            while tokio::fs::metadata(ancestor).await.is_err() {
                ancestor = ancestor.parent().ok_or_else(|| err("fs.write: bad path"))?;
            }
            let canonical_ancestor = contained_path(&config.root, ancestor).await?;
            let _ = canonical_ancestor;
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| err(&format!("fs.write: {e}")))?;
        }
    }
    let full = contained_path(&config.root, &joined).await?;
    let previous = tokio::fs::read(&full).await.ok();
    let existed = previous.is_some();
    let backup = match &previous {
        Some(bytes) => serde_json::json!({
            "existed": true,
            "bytes": bytes.len(),
            "hash": hash_bytes(bytes),
        }),
        None => serde_json::json!({ "existed": false }),
    };
    tokio::fs::write(&full, content.as_bytes())
        .await
        .map_err(|e| err(&format!("fs.write: {e}")))?;
    let written = tokio::fs::read(&full)
        .await
        .map_err(|e| err(&format!("fs.write verify: {e}")))?;
    Ok(serde_json::json!({
        "path": path,
        "bytes": written.len(),
        "hash": hash_bytes(&written),
        "overwrote": existed,
        "backup": backup,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn root() -> (tempfile::TempDir, SandboxConfig) {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = SandboxConfig {
            root: dir.path().to_path_buf(),
            ..SandboxConfig::default()
        };
        (dir, config)
    }

    #[tokio::test]
    async fn read_list_search_behave_safely() {
        let (_dir, config) = root().await;
        tokio::fs::write(config.root.join("a.txt"), b"hello world")
            .await
            .expect("write");
        tokio::fs::create_dir(config.root.join("sub"))
            .await
            .expect("mkdir");
        tokio::fs::write(config.root.join("sub/b.txt"), b"hello again")
            .await
            .expect("write");
        let read = fs_read(&config, "a.txt").await.expect("read");
        assert_eq!(read["bytes"], 11);
        assert!(read["hash"].as_str().unwrap_or("").starts_with("sha256:"));
        assert!(read["text"].as_str().unwrap_or("").contains("hello"));
        let list = fs_list(&config, ".", 1).await.expect("list");
        assert!(list["entries"].as_array().unwrap_or(&vec![]).len() >= 2);
        let search = fs_search(&config, "hello", ".", 20).await.expect("search");
        assert_eq!(search["matches"].as_array().unwrap_or(&vec![]).len(), 2);
        assert!(fs_read(&config, "../escape").await.is_err());
    }

    #[tokio::test]
    async fn dry_run_produces_no_side_effects() {
        let (_dir, config) = root().await;
        let planned = fs_write_plan("new.txt", "content", false);
        assert_eq!(planned["planned"], true);
        assert!(!config.root.join("new.txt").exists());
    }
}
