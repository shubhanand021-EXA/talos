//! Task checkpoints for crash recovery.
//!
//! Every N tool calls (default [`DEFAULT_CHECKPOINT_EVERY`], see the
//! `tool_runtime` invoke hook), the executor snapshots the task into the
//! evidence ledger (`kind = task_checkpoint`): task status, event-log chain
//! head, a bounded workspace digest, and the tool-receipt count. On
//! unclean startup, [`run_startup_recovery`](crate::run_startup_recovery)
//! resumes `executing` tasks that have a checkpoint (`queued` with reason
//! `resumed_from_checkpoint`) and fails those without one
//! (`crash_recovery_failed`).
//!
//! Resume re-queues the task with its checkpoint attached — the worker
//! restarts the attempt. Idempotent tools make this safe; non-idempotent
//! side effects are the documented residual risk (the checkpoint tells the
//! operator exactly where execution stopped).

use std::path::Path;

use talos_common::{Result, TalosError};

use crate::db::EvidenceDb;
use crate::{get_task, insert_evidence, transition_evidence_status, NewEvidence};

/// Default tool-call interval between checkpoints (overridable at runtime
/// via `TALOS_CHECKPOINT_EVERY`; `0` disables checkpointing).
pub const DEFAULT_CHECKPOINT_EVERY: u64 = 5;
/// Evidence kind for checkpoint rows.
pub const CHECKPOINT_KIND: &str = "task_checkpoint";
/// Cap on files hashed into a workspace digest (bounded, deterministic).
pub const WORKSPACE_DIGEST_FILE_CAP: usize = 500;

/// A deserialized task checkpoint.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TaskCheckpoint {
    /// Evidence row id holding the checkpoint.
    pub id: String,
    /// Owning task.
    pub task_id: String,
    /// When the checkpoint was recorded (RFC 3339).
    pub created_at: String,
    /// Task status at checkpoint time.
    pub status: String,
    /// Event-log chain head sequence (`-1` when the log is empty).
    pub chain_head_seq: i64,
    /// Event-log chain head hash (`GENESIS` when the log is empty).
    pub chain_head_hash: String,
    /// Workspace root snapshotted, if any.
    pub workspace_root: Option<String>,
    /// `sha256:` digest of the bounded workspace listing, if snapshotted.
    pub workspace_digest: Option<String>,
    /// Tool receipts recorded for the task at checkpoint time.
    pub tool_receipt_count: i64,
}

fn bad(msg: &str) -> TalosError {
    TalosError::Storage(msg.to_owned())
}

/// Bounded, deterministic workspace digest: `sha256:` over up to
/// [`WORKSPACE_DIGEST_FILE_CAP`] `relpath:size:mtime` lines, sorted.
/// Returns `None` when the root is missing or unreadable.
fn workspace_digest(root: &Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) -> bool {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return false;
        };
        let mut entries: Vec<_> = entries.filter_map(|e| e.ok()).collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            if out.len() >= WORKSPACE_DIGEST_FILE_CAP {
                return true;
            }
            let path = entry.path();
            let rel = path.strip_prefix(base).unwrap_or(&path).to_string_lossy();
            // Skip SQLite sidecars: locking/heat, not task state.
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if matches!(ext, "wal" | "shm" | "journal") {
                    continue;
                }
            }
            let meta = entry.metadata().ok();
            let (kind, size, mtime) = match (&meta, meta.as_ref().and_then(|m| m.modified().ok())) {
                _ if path.is_dir() => {
                    if !walk(&path, base, out) {
                        return false;
                    }
                    continue;
                }
                (Some(m), Some(t)) => (
                    "f",
                    m.len(),
                    t.duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0),
                ),
                _ => continue,
            };
            out.push(format!("{rel}:{kind}:{size}:{mtime}"));
        }
        true
    }
    let mut lines = Vec::new();
    if !walk(root, root, &mut lines) {
        return None;
    }
    lines.sort();
    let mut hasher = Sha256::new();
    for line in &lines {
        hasher.update(line.as_bytes());
        hasher.update(b"\n");
    }
    Some(format!("sha256:{}", hex::encode(hasher.finalize())))
}

/// Record a checkpoint for `task_id` and return its evidence id.
pub async fn record_task_checkpoint(
    db: &EvidenceDb,
    task_id: &str,
    workspace_root: Option<&Path>,
) -> Result<String> {
    let task_status = match get_task(db, task_id).await? {
        Some(task) => task.status,
        // Ad-hoc tool calls may reference tasks outside the durable queue
        // (diagnostics, probes); record honestly instead of failing.
        None => "unknown".to_owned(),
    };
    let head: Option<(i64, String)> =
        sqlx::query_as("SELECT seq, hash FROM event_log ORDER BY seq DESC LIMIT 1")
            .fetch_optional(db.pool())
            .await
            .map_err(|e| bad(&format!("chain head: {e}")))?;
    let (chain_head_seq, chain_head_hash) = head.unwrap_or((-1, crate::GENESIS_HASH.to_owned()));
    let tool_receipt_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM evidence_records WHERE kind = 'tool_receipt' AND task_id = ?",
    )
    .bind(task_id)
    .fetch_one(db.pool())
    .await
    .map_err(|e| bad(&format!("tool receipt count: {e}")))?;
    let (workspace_root_str, workspace_digest_str) = match workspace_root {
        Some(root) => (
            Some(root.to_string_lossy().into_owned()),
            workspace_digest(root),
        ),
        None => (None, None),
    };
    let id = format!("checkpoint-{task_id}-{}", uuid::Uuid::new_v4());
    insert_evidence(
        db,
        NewEvidence {
            id: id.clone(),
            task_id: Some(task_id.to_owned()),
            agent_id: None,
            session_id: Some("session-local".to_owned()),
            kind: CHECKPOINT_KIND.to_owned(),
            label: Some("task checkpoint".to_owned()),
            summary: Some(format!(
                "checkpoint of {task_id} at {task_status} (chain #{chain_head_seq})"
            )),
            uri: None,
            mime_type: None,
            payload: serde_json::json!({
                "task_id": task_id,
                "status": task_status,
                "chain_head_seq": chain_head_seq,
                "chain_head_hash": chain_head_hash,
                "workspace_root": workspace_root_str,
                "workspace_digest": workspace_digest_str,
                "tool_receipt_count": tool_receipt_count,
            }),
        },
    )
    .await?;
    transition_evidence_status(db, &id, "verified").await?;
    Ok(id)
}

/// Latest checkpoint for a task, if any.
pub async fn latest_checkpoint(db: &EvidenceDb, task_id: &str) -> Result<Option<TaskCheckpoint>> {
    let row: Option<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT id, created_at, payload_json FROM evidence_records WHERE kind = 'task_checkpoint' AND task_id = ? ORDER BY created_at DESC, id DESC LIMIT 1",
    )
    .bind(task_id)
    .fetch_optional(db.pool())
    .await
    .map_err(|e| bad(&format!("latest checkpoint: {e}")))?;
    let Some((id, created_at, payload_json)) = row else {
        return Ok(None);
    };
    let payload: serde_json::Value = payload_json
        .as_deref()
        .and_then(|raw| serde_json::from_str(raw).ok())
        .unwrap_or(serde_json::Value::Null);
    let get_str = |key: &str| payload.get(key).and_then(|v| v.as_str()).map(str::to_owned);
    let get_i64 = |key: &str| payload.get(key).and_then(serde_json::Value::as_i64);
    Ok(Some(TaskCheckpoint {
        id,
        task_id: task_id.to_owned(),
        created_at,
        status: get_str("status").unwrap_or_default(),
        chain_head_seq: get_i64("chain_head_seq").unwrap_or(-1),
        chain_head_hash: get_str("chain_head_hash")
            .unwrap_or_else(|| crate::GENESIS_HASH.to_owned()),
        workspace_root: get_str("workspace_root"),
        workspace_digest: get_str("workspace_digest"),
        tool_receipt_count: get_i64("tool_receipt_count").unwrap_or(0),
    }))
}

/// Checkpoint when at least `every` tool receipts landed since the last one.
///
/// Returns the new checkpoint id, or `None` when the interval has not
/// elapsed. `every == 0` disables checkpointing.
pub async fn maybe_checkpoint_task(
    db: &EvidenceDb,
    task_id: &str,
    workspace_root: Option<&Path>,
    every: u64,
) -> Result<Option<String>> {
    if every == 0 {
        return Ok(None);
    }
    let total: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM evidence_records WHERE kind = 'tool_receipt' AND task_id = ?",
    )
    .bind(task_id)
    .fetch_one(db.pool())
    .await
    .map_err(|e| bad(&format!("tool receipt count: {e}")))?;
    let last = match latest_checkpoint(db, task_id).await? {
        Some(checkpoint) => checkpoint.tool_receipt_count,
        None => 0,
    };
    if total.saturating_sub(last) < every as i64 {
        return Ok(None);
    }
    Ok(Some(
        record_task_checkpoint(db, task_id, workspace_root).await?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::EnqueueParams;

    async fn db_with_task(id: &str) -> EvidenceDb {
        let db = EvidenceDb::connect_memory()
            .await
            .expect("memory db connects");
        crate::enqueue_task(
            &db,
            EnqueueParams {
                id,
                title: "checkpoint probe",
                description: None,
                priority: "normal",
                max_attempts: 3,
                timeout_ms: 30_000,
                backoff_ms: 500,
                scenario: "quick_success",
                parent_task_id: None,
            },
        )
        .await
        .expect("enqueue works");
        db
    }

    async fn tool_receipt(db: &EvidenceDb, task_id: &str, n: u64) {
        for _ in 0..n {
            crate::insert_evidence(
                db,
                NewEvidence {
                    id: format!("tool-ev-{task_id}-{n}-{}_{}", n, uuid::Uuid::new_v4()),
                    task_id: Some(task_id.to_owned()),
                    agent_id: None,
                    session_id: None,
                    kind: "tool_receipt".to_owned(),
                    label: None,
                    summary: None,
                    uri: None,
                    mime_type: None,
                    payload: serde_json::json!({}),
                },
            )
            .await
            .expect("receipt inserts");
        }
    }

    #[tokio::test]
    async fn record_and_read_checkpoint() {
        let db = db_with_task("t-check").await;
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join("notes.md"), b"hello").expect("writes");
        let id = record_task_checkpoint(&db, "t-check", Some(dir.path()))
            .await
            .expect("records");
        let checkpoint = latest_checkpoint(&db, "t-check")
            .await
            .expect("reads")
            .expect("exists");
        assert_eq!(checkpoint.id, id);
        assert_eq!(checkpoint.status, "queued");
        assert_eq!(checkpoint.tool_receipt_count, 0);
        assert_eq!(
            checkpoint.workspace_root.as_deref(),
            Some(dir.path().to_string_lossy().as_ref())
        );
        assert!(checkpoint
            .workspace_digest
            .as_deref()
            .unwrap_or_default()
            .starts_with("sha256:"));
        assert!(checkpoint.chain_head_seq >= 0);
    }

    #[tokio::test]
    async fn checkpoint_every_n_tool_calls() {
        let db = db_with_task("t-every").await;
        assert!(maybe_checkpoint_task(&db, "t-every", None, 5)
            .await
            .expect("checks")
            .is_none());
        tool_receipt(&db, "t-every", 4).await;
        assert!(maybe_checkpoint_task(&db, "t-every", None, 5)
            .await
            .expect("checks")
            .is_none());
        tool_receipt(&db, "t-every", 1).await;
        let id = maybe_checkpoint_task(&db, "t-every", None, 5)
            .await
            .expect("checks")
            .expect("checkpoints at 5");
        let checkpoint = latest_checkpoint(&db, "t-every")
            .await
            .expect("reads")
            .expect("exists");
        assert_eq!(checkpoint.id, id);
        assert_eq!(checkpoint.tool_receipt_count, 5);
        // Interval restarts after each checkpoint.
        tool_receipt(&db, "t-every", 4).await;
        assert!(maybe_checkpoint_task(&db, "t-every", None, 5)
            .await
            .expect("checks")
            .is_none());
        assert!(maybe_checkpoint_task(&db, "t-every", None, 0)
            .await
            .expect("checks")
            .is_none());
    }

    #[tokio::test]
    async fn missing_task_records_unknown_status() {
        let db = db_with_task("t-other").await;
        assert!(latest_checkpoint(&db, "t-missing")
            .await
            .expect("reads")
            .is_none());
        let id = record_task_checkpoint(&db, "t-missing", None)
            .await
            .expect("records with unknown status");
        let checkpoint = latest_checkpoint(&db, "t-missing")
            .await
            .expect("reads")
            .expect("exists");
        assert_eq!(checkpoint.id, id);
        assert_eq!(checkpoint.status, "unknown");
    }

    #[test]
    fn workspace_digest_is_bounded_and_deterministic() {
        let dir = tempfile::tempdir().expect("temp dir");
        for i in 0..10 {
            std::fs::write(dir.path().join(format!("f{i}.txt")), b"x").expect("writes");
        }
        let first = workspace_digest(dir.path()).expect("digests");
        let second = workspace_digest(dir.path()).expect("digests");
        assert_eq!(first, second);
        assert!(first.starts_with("sha256:"));
        assert!(workspace_digest(Path::new("/nonexistent-talos-probe")).is_none());
    }
}
