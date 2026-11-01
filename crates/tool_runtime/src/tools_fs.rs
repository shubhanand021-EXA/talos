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

/// Snapshot of a file before an edit (for before/after artifacts).
pub async fn snapshot_file(config: &SandboxConfig, path: &str) -> Result<serde_json::Value> {
    let joined = join_under_root(&config.root, path)?;
    let full = contained_path(&config.root, &joined).await?;
    match tokio::fs::read(&full).await {
        Ok(bytes) => Ok(serde_json::json!({
            "path": path,
            "bytes": bytes.len(),
            "hash": hash_bytes(&bytes),
            "existed": true,
        })),
        Err(_) => Ok(serde_json::json!({ "path": path, "existed": false })),
    }
}

/// One parsed unified-diff hunk.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Hunk {
    /// 1-based old start line.
    old_start: usize,
    /// Old line count.
    old_len: usize,
    /// 1-based new start line.
    new_start: usize,
    /// New line count.
    new_len: usize,
    /// Body lines (` ` context, `-` removal, `+` addition).
    body: Vec<String>,
}

/// Parse a unified diff into hunks (headers `---`/`+++` ignored).
fn parse_unified_diff(patch: &str) -> Result<Vec<Hunk>> {
    let mut hunks = Vec::new();
    let mut current: Option<Hunk> = None;
    for raw in patch.lines() {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if line.starts_with("---") || line.starts_with("+++") {
            continue;
        }
        if let Some(header) = line.strip_prefix("@@") {
            let end = header
                .find("@@")
                .ok_or_else(|| err("fs.edit: bad hunk header"))?;
            let ranges = header[..end].trim();
            let mut parts = ranges.split_whitespace();
            let old = parts
                .next()
                .ok_or_else(|| err("fs.edit: bad hunk header"))?;
            let new = parts
                .next()
                .ok_or_else(|| err("fs.edit: bad hunk header"))?;
            let (old_start, old_len) = parse_range(old)?;
            let (new_start, new_len) = parse_range(new)?;
            if let Some(hunk) = current.take() {
                hunks.push(hunk);
            }
            current = Some(Hunk {
                old_start,
                old_len,
                new_start,
                new_len,
                body: Vec::new(),
            });
            continue;
        }
        if line.starts_with(' ') || line.starts_with('-') || line.starts_with('+') {
            current
                .as_mut()
                .ok_or_else(|| err("fs.edit: hunk body outside header"))?
                .body
                .push(line.to_owned());
        } else if line.trim().is_empty() {
            // Tolerate blank lines as context separators inside hunks.
            if let Some(hunk) = current.as_mut() {
                hunk.body.push(format!(" {line}"));
            }
        } else if line.starts_with("diff ")
            || line.starts_with("index ")
            || line.starts_with("new file")
        {
            continue;
        } else {
            return Err(err("fs.edit: unrecognized patch line"));
        }
    }
    if let Some(hunk) = current.take() {
        hunks.push(hunk);
    }
    if hunks.is_empty() {
        return Err(err("fs.edit: no hunks in patch"));
    }
    Ok(hunks)
}

fn parse_range(range: &str) -> Result<(usize, usize)> {
    let inner = range
        .strip_prefix(['-', '+'])
        .ok_or_else(|| err("fs.edit: bad range"))?;
    if let Some((start, len)) = inner.split_once(',') {
        let start: usize = start.parse().map_err(|_| err("fs.edit: bad range"))?;
        let len: usize = len.parse().map_err(|_| err("fs.edit: bad range"))?;
        Ok((start.max(1), len))
    } else {
        let start: usize = inner.parse().map_err(|_| err("fs.edit: bad range"))?;
        Ok((start.max(1), 1))
    }
}

/// Apply parsed hunks to `lines`; verifies context/removals first.
fn apply_hunks(lines: &[String], hunks: &[Hunk]) -> Result<Vec<String>> {
    let mut out = lines.to_vec();
    // Apply back-to-front so earlier offsets stay valid.
    let mut ordered: Vec<&Hunk> = hunks.iter().collect();
    ordered.sort_by_key(|a| std::cmp::Reverse(a.old_start));
    for hunk in ordered {
        let start = hunk.old_start.saturating_sub(1);
        let mut cursor = start;
        let mut replacement: Vec<String> = Vec::new();
        for body_line in &hunk.body {
            let (marker, content) = body_line.split_at(1);
            match marker {
                " " => {
                    let current = out
                        .get(cursor)
                        .ok_or_else(|| err("fs.edit: context out of range"))?;
                    if current != content {
                        return Err(err("fs.edit: context mismatch (patch does not apply)"));
                    }
                    replacement.push(current.clone());
                    cursor += 1;
                }
                "-" => {
                    let current = out
                        .get(cursor)
                        .ok_or_else(|| err("fs.edit: removal out of range"))?;
                    if current != content {
                        return Err(err("fs.edit: removal mismatch (patch does not apply)"));
                    }
                    cursor += 1;
                }
                "+" => replacement.push(content.to_owned()),
                _ => return Err(err("fs.edit: bad hunk line")),
            }
        }
        if cursor.saturating_sub(start) != hunk.old_len {
            return Err(err("fs.edit: hunk length mismatch"));
        }
        out.splice(start..cursor, replacement);
    }
    Ok(out)
}

/// Apply a unified diff patch to a file (live): validates, backs up, applies.
pub async fn fs_edit(config: &SandboxConfig, path: &str, patch: &str) -> Result<serde_json::Value> {
    if patch.len() > config.max_bytes {
        return Err(err("fs.edit: patch exceeds max_bytes"));
    }
    let joined = join_under_root(&config.root, path)?;
    let full = contained_path(&config.root, &joined).await?;
    let previous = tokio::fs::read(&full)
        .await
        .map_err(|_| err("fs.edit: file does not exist (use fs.create first)"))?;
    let text =
        String::from_utf8(previous.clone()).map_err(|_| err("fs.edit: file is not UTF-8"))?;
    if text.len() > config.max_bytes {
        return Err(err("fs.edit: file exceeds max_bytes"));
    }
    let hunks = parse_unified_diff(patch)?;
    let lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let trailing_newline = text.ends_with('\n');
    let updated = apply_hunks(&lines, &hunks)?;
    let mut next = updated.join("\n");
    if trailing_newline || text.is_empty() {
        next.push('\n');
    }
    let backup = serde_json::json!({
        "existed": true,
        "bytes": previous.len(),
        "hash": hash_bytes(&previous),
    });
    tokio::fs::write(&full, next.as_bytes())
        .await
        .map_err(|e| err(&format!("fs.edit: {e}")))?;
    Ok(serde_json::json!({
        "path": path,
        "bytes": next.len(),
        "hash": hash_bytes(next.as_bytes()),
        "hunks": hunks.len(),
        "backup": backup,
    }))
}

/// Create a new file with content.
pub async fn fs_create(
    config: &SandboxConfig,
    path: &str,
    content: &str,
    overwrite: bool,
) -> Result<serde_json::Value> {
    if content.len() > config.max_bytes {
        return Err(err("fs.create: content exceeds max_bytes"));
    }
    let joined = join_under_root(&config.root, path)?;
    if let Some(parent) = joined.parent() {
        if tokio::fs::metadata(parent).await.is_err() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| err(&format!("fs.create: {e}")))?;
        }
    }
    let full = contained_path(&config.root, &joined).await?;
    let previous = tokio::fs::read(&full).await.ok();
    if previous.is_some() && !overwrite {
        return Err(err("fs.create: file exists (set overwrite to replace)"));
    }
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
        .map_err(|e| err(&format!("fs.create: {e}")))?;
    Ok(serde_json::json!({
        "path": path,
        "bytes": content.len(),
        "hash": hash_bytes(content.as_bytes()),
        "overwrote": previous.is_some(),
        "backup": backup,
    }))
}

/// Delete a file (backup returned for the compensation plan).
pub async fn fs_delete(config: &SandboxConfig, path: &str) -> Result<serde_json::Value> {
    let joined = join_under_root(&config.root, path)?;
    let full = contained_path(&config.root, &joined).await?;
    let previous = tokio::fs::read(&full)
        .await
        .map_err(|_| err("fs.delete: file does not exist"))?;
    let backup = serde_json::json!({
        "existed": true,
        "bytes": previous.len(),
        "hash": hash_bytes(&previous),
        "content": String::from_utf8(previous.clone()).ok(),
    });
    tokio::fs::remove_file(&full)
        .await
        .map_err(|e| err(&format!("fs.delete: {e}")))?;
    Ok(serde_json::json!({
        "path": path,
        "deleted": true,
        "backup": backup,
    }))
}

/// Move/rename a file (backs up an overwritten destination).
pub async fn fs_move(
    config: &SandboxConfig,
    from: &str,
    to: &str,
    overwrite: bool,
) -> Result<serde_json::Value> {
    let joined_from = join_under_root(&config.root, from)?;
    let full_from = contained_path(&config.root, &joined_from).await?;
    let bytes = tokio::fs::read(&full_from)
        .await
        .map_err(|_| err("fs.move: source does not exist"))?;
    let joined_to = join_under_root(&config.root, to)?;
    if let Some(parent) = joined_to.parent() {
        if tokio::fs::metadata(parent).await.is_err() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| err(&format!("fs.move: {e}")))?;
        }
    }
    let full_to = contained_path(&config.root, &joined_to).await?;
    let dest_previous = tokio::fs::read(&full_to).await.ok();
    if dest_previous.is_some() && !overwrite {
        return Err(err(
            "fs.move: destination exists (set overwrite to replace)",
        ));
    }
    let backup = match &dest_previous {
        Some(previous) => serde_json::json!({
            "existed": true,
            "bytes": previous.len(),
            "hash": hash_bytes(previous),
        }),
        None => serde_json::json!({ "existed": false }),
    };
    tokio::fs::rename(&full_from, &full_to)
        .await
        .map_err(|e| err(&format!("fs.move: {e}")))?;
    Ok(serde_json::json!({
        "from": from,
        "to": to,
        "bytes": bytes.len(),
        "hash": hash_bytes(&bytes),
        "backup": backup,
    }))
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

    #[tokio::test]
    async fn edit_applies_unified_diff_with_backup() {
        let (_dir, config) = root().await;
        tokio::fs::write(config.root.join("code.txt"), b"line1\nline2\nline3\n")
            .await
            .expect("write");
        let patch = "@@ -1,3 +1,3 @@\n line1\n-line2\n+lineTWO\n line3\n";
        let out = fs_edit(&config, "code.txt", patch).await.expect("edit");
        assert_eq!(out["hunks"], 1);
        assert!(out["backup"]["existed"].as_bool().unwrap_or(false));
        let text = tokio::fs::read_to_string(config.root.join("code.txt"))
            .await
            .expect("read");
        assert!(text.contains("lineTWO"));
        // A patch against stale content is rejected before mutation.
        let stale = "@@ -1,3 +1,3 @@\n line1\n-lineX\n+lineY\n line3\n";
        assert!(fs_edit(&config, "code.txt", stale).await.is_err());
        let text = tokio::fs::read_to_string(config.root.join("code.txt"))
            .await
            .expect("read");
        assert!(text.contains("lineTWO"));
    }

    #[tokio::test]
    async fn create_move_delete_round_trip() {
        let (_dir, config) = root().await;
        let created = fs_create(&config, "new/file.txt", "hello", false)
            .await
            .expect("create");
        assert_eq!(created["overwrote"], false);
        assert!(fs_create(&config, "new/file.txt", "again", false)
            .await
            .is_err());
        let moved = fs_move(&config, "new/file.txt", "moved.txt", false)
            .await
            .expect("move");
        assert_eq!(moved["to"], "moved.txt");
        assert!(!config.root.join("new/file.txt").exists());
        let deleted = fs_delete(&config, "moved.txt").await.expect("delete");
        assert_eq!(deleted["deleted"], true);
        assert!(deleted["backup"]["existed"].as_bool().unwrap_or(false));
        assert!(fs_delete(&config, "moved.txt").await.is_err());
    }
}
