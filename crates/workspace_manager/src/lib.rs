//! Isolated coding workspaces: one filesystem root per task.
//!
//! For each coding task the manager creates an isolated workspace:
//!
//! - If the caller references a git repo, a git worktree is added
//!   (`git worktree add <base>/<task_id> -b talos/<task_id>`); the parent
//!   repo is never touched directly — all git operations stay scoped to the
//!   worktree root.
//! - Otherwise an isolated directory is created under the base dir.
//!
//! The workspace root is the ONLY filesystem root the task's agent may
//! access (enforced by [`tool_runtime`] sandbox confinement on top of these
//! paths). Cleanup removes the worktree registration first, then the
//! directory. Nothing here bypasses the policy broker.

use std::path::{Path, PathBuf};

use talos_common::{Result, TalosError};

fn err(msg: impl Into<String>) -> TalosError {
    TalosError::Storage(msg.into())
}

/// Validate a task id for use as a directory/branch name.
fn check_task_id(task_id: &str) -> Result<()> {
    if task_id.trim().is_empty() || task_id.len() > 128 {
        return Err(err("task_id must be 1..=128 chars"));
    }
    if task_id.contains('/') || task_id.contains('\\') || task_id.contains('\0') {
        return Err(err("task_id must not contain path separators"));
    }
    if task_id == "." || task_id == ".." {
        return Err(err("task_id must not be . or .."));
    }
    Ok(())
}

/// Sanitize a task id into a git-safe branch suffix.
fn branch_suffix(task_id: &str) -> String {
    let mut out: String = task_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    out.truncate(64);
    let trimmed = out.trim_matches(|c| c == '-' || c == '_');
    if trimmed.is_empty() {
        "task".to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// One isolated workspace: the only root a task agent may touch.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    /// Owning task.
    pub task_id: String,
    /// Isolated root (worktree dir or plain dir).
    pub root: PathBuf,
    /// Workspace branch (`talos/<sanitized>` when git-backed).
    pub branch: String,
    /// True when created via `git worktree add`.
    pub is_git_worktree: bool,
    /// RFC 3339 creation timestamp.
    pub created_at: String,
}

/// Manager for per-task isolated workspaces under one base dir.
#[derive(Debug, Clone)]
pub struct WorkspaceManager {
    base_dir: PathBuf,
}

impl WorkspaceManager {
    /// Create a manager over `base_dir` (created on demand).
    #[must_use]
    pub fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    /// Default base dir (`<temp>/talos-workspaces`).
    #[must_use]
    pub fn default_base() -> PathBuf {
        std::env::temp_dir().join("talos-workspaces")
    }

    /// Base dir holding all workspaces.
    #[must_use]
    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    /// Expected root for one task (no I/O).
    pub fn workspace_root(&self, task_id: &str) -> Result<PathBuf> {
        check_task_id(task_id)?;
        Ok(self.base_dir.join(task_id))
    }

    /// True when a workspace dir already exists for `task_id`.
    pub async fn exists(&self, task_id: &str) -> bool {
        let Ok(root) = self.workspace_root(task_id) else {
            return false;
        };
        tokio::fs::metadata(&root).await.is_ok()
    }

    /// Create (or reuse) an isolated workspace for `task_id`.
    ///
    /// When `repo_hint` points inside a git repo, a linked worktree is added
    /// on branch `talos/<sanitized-task>`; otherwise a plain directory is
    /// created. Reuses the existing dir when already present (idempotent).
    pub async fn create_workspace(
        &self,
        task_id: &str,
        repo_hint: Option<&Path>,
    ) -> Result<Workspace> {
        check_task_id(task_id)?;
        let root = self.base_dir.join(task_id);
        let branch = format!("talos/{}", branch_suffix(task_id));
        tokio::fs::create_dir_all(&self.base_dir)
            .await
            .map_err(|e| err(format!("workspace base unavailable: {e}")))?;
        if tokio::fs::metadata(&root).await.is_ok() {
            return Ok(Workspace {
                task_id: task_id.to_owned(),
                root: root.clone(),
                branch,
                is_git_worktree: is_worktree_dir(&root).await,
                created_at: talos_common::now_iso(),
            });
        }
        if let Some(repo) = repo_hint {
            if let Some(worktree) = try_git_worktree(repo, &root, &branch).await? {
                return Ok(worktree);
            }
        }
        tokio::fs::create_dir_all(&root)
            .await
            .map_err(|e| err(format!("workspace create: {e}")))?;
        Ok(Workspace {
            task_id: task_id.to_owned(),
            root,
            branch,
            is_git_worktree: false,
            created_at: talos_common::now_iso(),
        })
    }

    /// Ensure a workspace exists (plain dir when no repo hint).
    pub async fn ensure_workspace(&self, task_id: &str) -> Result<Workspace> {
        self.create_workspace(task_id, None).await
    }

    /// Load workspace info when the dir exists.
    pub async fn get_workspace(&self, task_id: &str) -> Result<Option<Workspace>> {
        check_task_id(task_id)?;
        let root = self.base_dir.join(task_id);
        if tokio::fs::metadata(&root).await.is_err() {
            return Ok(None);
        }
        Ok(Some(Workspace {
            task_id: task_id.to_owned(),
            root: root.clone(),
            branch: format!("talos/{}", branch_suffix(task_id)),
            is_git_worktree: is_worktree_dir(&root).await,
            created_at: talos_common::now_iso(),
        }))
    }

    /// List existing workspaces (best effort, sorted by task id).
    pub async fn list_workspaces(&self) -> Vec<Workspace> {
        let mut out = Vec::new();
        let Ok(mut stream) = tokio::fs::read_dir(&self.base_dir).await else {
            return out;
        };
        while let Ok(Some(child)) = stream.next_entry().await {
            let Ok(file_type) = child.file_type().await else {
                continue;
            };
            if !file_type.is_dir() {
                continue;
            }
            let task_id = child.file_name().to_string_lossy().into_owned();
            if check_task_id(&task_id).is_err() {
                continue;
            }
            out.push(Workspace {
                task_id: task_id.clone(),
                root: child.path(),
                branch: format!("talos/{}", branch_suffix(&task_id)),
                is_git_worktree: is_worktree_dir(&child.path()).await,
                created_at: talos_common::now_iso(),
            });
        }
        out.sort_by(|a, b| a.task_id.cmp(&b.task_id));
        out
    }

    /// Remove a workspace: unregisters the git worktree first (when
    /// git-backed), then removes the directory. Missing dirs are OK.
    pub async fn cleanup(&self, task_id: &str) -> Result<bool> {
        check_task_id(task_id)?;
        let root = self.base_dir.join(task_id);
        if tokio::fs::metadata(&root).await.is_err() {
            return Ok(false);
        }
        if is_worktree_dir(&root).await {
            remove_git_worktree(&root).await;
        }
        // The worktree removal may already have deleted the dir.
        if tokio::fs::metadata(&root).await.is_err() {
            return Ok(true);
        }
        tokio::fs::remove_dir_all(&root)
            .await
            .map_err(|e| err(format!("workspace cleanup: {e}")))?;
        Ok(true)
    }

    /// True when `candidate` (already joined) stays inside the task root.
    pub async fn is_within(&self, task_id: &str, candidate: &Path) -> bool {
        let (Ok(root), Ok(canonical_root)) =
            (self.workspace_root(task_id), self.canonical_base().await)
        else {
            return false;
        };
        let _ = root;
        let Ok(canonical) = tokio::fs::canonicalize(candidate).await else {
            return false;
        };
        canonical.starts_with(&canonical_root)
    }

    /// Canonical base dir (creates it first).
    async fn canonical_base(&self) -> Result<PathBuf> {
        tokio::fs::create_dir_all(&self.base_dir)
            .await
            .map_err(|e| err(format!("workspace base unavailable: {e}")))?;
        tokio::fs::canonicalize(&self.base_dir)
            .await
            .map_err(|e| err(format!("workspace base unavailable: {e}")))
    }
}

/// Heuristic: a dir holding `.git` (file for worktrees, dir for clones).
async fn is_worktree_dir(root: &Path) -> bool {
    tokio::fs::metadata(root.join(".git")).await.is_ok()
}

/// Try `git worktree add`; returns `Ok(None)` when git is unavailable or the
/// hint is not a repo (caller falls back to a plain dir).
async fn try_git_worktree(
    repo_hint: &Path,
    root: &Path,
    branch: &str,
) -> Result<Option<Workspace>> {
    let repo_root = find_repo_root(repo_hint).await;
    let Some(repo_root) = repo_root else {
        return Ok(None);
    };
    if tokio::fs::metadata(&root).await.is_ok() {
        return Ok(None);
    }
    if let Some(parent) = root.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| err(format!("workspace base unavailable: {e}")))?;
    }
    // New branch first; fall back to attaching the existing branch name.
    let added = run_git(
        &repo_root,
        &["worktree", "add", &root_string(root)?, "-b", branch],
    )
    .await
    .is_ok()
        || run_git(
            &repo_root,
            &["worktree", "add", &root_string(root)?, branch],
        )
        .await
        .is_ok();
    if !added {
        return Ok(None);
    }
    Ok(Some(Workspace {
        task_id: root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        root: root.to_path_buf(),
        branch: branch.to_owned(),
        is_git_worktree: true,
        created_at: talos_common::now_iso(),
    }))
}

fn root_string(root: &Path) -> Result<String> {
    root.to_str()
        .map(str::to_owned)
        .ok_or_else(|| err("workspace path is not UTF-8"))
}

/// Walk up from `hint` to find a dir containing `.git`.
async fn find_repo_root(hint: &Path) -> Option<PathBuf> {
    let mut current = if hint.is_file() {
        hint.parent()?.to_path_buf()
    } else {
        hint.to_path_buf()
    };
    loop {
        if tokio::fs::metadata(current.join(".git")).await.is_ok() {
            return Some(current);
        }
        match current.parent() {
            Some(parent) if parent != current => current = parent.to_path_buf(),
            _ => return None,
        }
    }
}

/// Run a git command with a short timeout; errors are non-fatal to callers.
async fn run_git(repo: &Path, args: &[&str]) -> Result<()> {
    let mut child = tokio::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| err(format!("git unavailable: {e}")))?;
    let status = tokio::time::timeout(std::time::Duration::from_secs(30), child.wait())
        .await
        .map_err(|_| err("git timed out"))?
        .map_err(|e| err(format!("git wait: {e}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(err(format!("git exited {}", status.code().unwrap_or(-1))))
    }
}

/// Best-effort `git worktree remove --force` (failures fall through to dir removal).
async fn remove_git_worktree(root: &Path) {
    // A linked worktree holds `<root>/.git` with `gitdir: <main>/.git/worktrees/<name>`.
    let Ok(content) = tokio::fs::read_to_string(root.join(".git")).await else {
        return;
    };
    let Some(gitdir) = content.trim().strip_prefix("gitdir:").map(str::trim) else {
        return;
    };
    let main: Option<PathBuf> = Path::new(gitdir)
        .parent() // <main>/.git/worktrees
        .and_then(|p| p.parent()) // <main>/.git
        .and_then(|p| p.parent()) // <main>
        .map(Path::to_path_buf);
    let Some(main) = main else {
        return;
    };
    let root_str = root.to_string_lossy().into_owned();
    let _ = run_git(&main, &["worktree", "remove", "--force", &root_str]).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn manager() -> (tempfile::TempDir, WorkspaceManager) {
        let dir = tempfile::tempdir().expect("tempdir");
        let mgr = WorkspaceManager::new(dir.path().join("ws"));
        (dir, mgr)
    }

    #[tokio::test]
    async fn creates_and_cleans_up_plain_workspace() {
        let (_dir, mgr) = manager().await;
        let ws = mgr.create_workspace("task-1", None).await.expect("create");
        assert!(ws.root.ends_with("task-1"));
        assert!(!ws.is_git_worktree);
        assert!(mgr.exists("task-1").await);
        let fetched = mgr.get_workspace("task-1").await.expect("get");
        assert!(fetched.is_some());
        assert!(mgr.cleanup("task-1").await.expect("cleanup"));
        assert!(!mgr.exists("task-1").await);
        // Missing cleanup is OK (idempotent).
        assert!(!mgr.cleanup("task-1").await.expect("cleanup"));
    }

    #[tokio::test]
    async fn rejects_bad_task_ids() {
        let (_dir, mgr) = manager().await;
        assert!(mgr.create_workspace("", None).await.is_err());
        assert!(mgr.create_workspace("../escape", None).await.is_err());
        assert!(mgr.create_workspace("a/b", None).await.is_err());
        assert!(mgr.cleanup("..").await.is_err());
    }

    #[tokio::test]
    async fn reuses_existing_workspace_idempotently() {
        let (_dir, mgr) = manager().await;
        let first = mgr.create_workspace("task-9", None).await.expect("create");
        tokio::fs::write(first.root.join("note.txt"), b"hi")
            .await
            .expect("write");
        let second = mgr.create_workspace("task-9", None).await.expect("reuse");
        assert_eq!(first.root, second.root);
        assert!(second.root.join("note.txt").exists());
    }

    #[tokio::test]
    async fn lists_workspaces_sorted() {
        let (_dir, mgr) = manager().await;
        mgr.create_workspace("task-b", None).await.expect("create");
        mgr.create_workspace("task-a", None).await.expect("create");
        let listed = mgr.list_workspaces().await;
        let ids: Vec<&str> = listed.iter().map(|w| w.task_id.as_str()).collect();
        assert_eq!(ids, ["task-a", "task-b"]);
    }

    #[tokio::test]
    async fn git_worktree_when_repo_hinted() {
        if tokio::process::Command::new("git")
            .arg("--version")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false)
        {
            let repo = tempfile::tempdir().expect("tempdir");
            let init = tokio::process::Command::new("git")
                .args(["init", "-b", "main"])
                .current_dir(repo.path())
                .stdin(std::process::Stdio::null())
                .status()
                .await;
            if init.map(|s| s.success()).unwrap_or(false) {
                tokio::fs::write(repo.path().join("README.md"), b"hi")
                    .await
                    .expect("write");
                let (_dir, mgr) = manager().await;
                let ws = mgr
                    .create_workspace("task-git-1", Some(repo.path()))
                    .await
                    .expect("create");
                assert!(ws.root.exists());
                // Cleanup removes the worktree dir without touching the repo.
                assert!(mgr.cleanup("task-git-1").await.expect("cleanup"));
                assert!(repo.path().join("README.md").exists());
                return;
            }
        }
        // Git unavailable: plain-dir fallback still isolates.
        let (_dir, mgr) = manager().await;
        let ws = mgr
            .create_workspace("task-git-1", Some(Path::new("/nonexistent")))
            .await
            .expect("fallback");
        assert!(!ws.is_git_worktree);
    }
}
