//! Safe parallel execution: concurrency caps, file locks, browser sessions.
//!
//! The Supervisor runs at most [`DEFAULT_MAX_CONCURRENT_AGENTS`] (configurable,
//! hard cap [`ABSOLUTE_MAX_CONCURRENT`]) sub-agents at once. Two isolation
//! rules prevent clobbering:
//!
//! - One writer per workspace path: the first agent to claim a path holds it
//!   until [`ConcurrencyController::release`]; the second queues behind a
//!   [`ConflictKind::FileLock`] and the Supervisor emits `ConflictDetected`.
//! - One browser session: only one agent may hold the live browser session at
//!   a time ([`ConflictKind::BrowserBusy`]).

use std::collections::{HashMap, HashSet};

use crate::delegation::DEFAULT_MAX_CONCURRENT_AGENTS;

/// Hard ceiling for concurrent sub-agents (Perplexity §2.6 max 4).
pub const ABSOLUTE_MAX_CONCURRENT: usize = 4;

/// What blocked an agent acquisition.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictKind {
    /// Another task already holds one of the requested paths.
    FileLock,
    /// Another task already holds the browser session.
    BrowserBusy,
    /// Concurrency budget exhausted.
    ConcurrencyExhausted,
}

/// Outcome of [`ConcurrencyController::try_acquire`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcquireOutcome {
    /// Slot granted; the task now holds its files (+ browser when asked).
    Acquired,
    /// Blocked: the Supervisor must queue the task and emit `ConflictDetected`.
    Conflict {
        /// What blocked the acquisition.
        kind: ConflictKind,
        /// Human detail (holder, path, ...).
        detail: String,
    },
}

/// Tracks active agents, file locks, and the browser session holder.
#[derive(Debug)]
pub struct ConcurrencyController {
    /// Configured cap (clamped to 1..=[`ABSOLUTE_MAX_CONCURRENT`]).
    max_concurrent: usize,
    /// Active task ids.
    active: HashSet<String>,
    /// Workspace path → holder task.
    file_locks: HashMap<String, String>,
    /// Task → paths it holds (for release).
    by_task: HashMap<String, Vec<String>>,
    /// Current browser session holder, if any.
    browser_holder: Option<String>,
}

impl ConcurrencyController {
    /// Create a controller (cap clamped to 1..=4).
    #[must_use]
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            max_concurrent: max_concurrent.clamp(1, ABSOLUTE_MAX_CONCURRENT),
            active: HashSet::new(),
            file_locks: HashMap::new(),
            by_task: HashMap::new(),
            browser_holder: None,
        }
    }

    /// Default controller (2 concurrent agents).
    #[must_use]
    pub fn default_controller() -> Self {
        Self::new(DEFAULT_MAX_CONCURRENT_AGENTS)
    }

    /// Configured cap.
    #[must_use]
    pub fn max_concurrent(&self) -> usize {
        self.max_concurrent
    }

    /// Active agent count.
    #[must_use]
    pub fn active_count(&self) -> usize {
        self.active.len()
    }

    /// Current browser holder, if any.
    #[must_use]
    pub fn browser_holder(&self) -> Option<&str> {
        self.browser_holder.as_deref()
    }

    /// Try to admit `task_id` with `files` and optional browser need.
    pub fn try_acquire(
        &mut self,
        task_id: &str,
        files: &[String],
        needs_browser: bool,
    ) -> AcquireOutcome {
        if self.active.contains(task_id) {
            return AcquireOutcome::Acquired;
        }
        if self.active.len() >= self.max_concurrent {
            return AcquireOutcome::Conflict {
                kind: ConflictKind::ConcurrencyExhausted,
                detail: format!(
                    "concurrency cap {} reached; {} queued",
                    self.max_concurrent, task_id
                ),
            };
        }
        for path in files {
            if let Some(holder) = self.file_locks.get(path) {
                return AcquireOutcome::Conflict {
                    kind: ConflictKind::FileLock,
                    detail: format!("path {path} held by {holder}; {task_id} queued"),
                };
            }
        }
        if needs_browser {
            if let Some(holder) = &self.browser_holder {
                return AcquireOutcome::Conflict {
                    kind: ConflictKind::BrowserBusy,
                    detail: format!("browser session held by {holder}; {task_id} queued"),
                };
            }
        }
        self.active.insert(task_id.to_owned());
        if !files.is_empty() {
            for path in files {
                self.file_locks.insert(path.clone(), task_id.to_owned());
            }
            self.by_task.insert(task_id.to_owned(), files.to_vec());
        }
        if needs_browser {
            self.browser_holder = Some(task_id.to_owned());
        }
        AcquireOutcome::Acquired
    }

    /// Release a task's slot, file locks, and browser hold (idempotent).
    pub fn release(&mut self, task_id: &str) {
        self.active.remove(task_id);
        if let Some(paths) = self.by_task.remove(task_id) {
            for path in paths {
                if self.file_locks.get(&path).is_some_and(|h| h == task_id) {
                    self.file_locks.remove(&path);
                }
            }
        }
        if self.browser_holder.as_deref() == Some(task_id) {
            self.browser_holder = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_concurrency() {
        let mut ctrl = ConcurrencyController::new(1);
        assert_eq!(ctrl.try_acquire("a", &[], false), AcquireOutcome::Acquired);
        let blocked = ctrl.try_acquire("b", &[], false);
        assert!(matches!(
            blocked,
            AcquireOutcome::Conflict {
                kind: ConflictKind::ConcurrencyExhausted,
                ..
            }
        ));
        ctrl.release("a");
        assert_eq!(ctrl.try_acquire("b", &[], false), AcquireOutcome::Acquired);
    }

    #[test]
    fn file_lock_conflict_queues_second_writer() {
        let mut ctrl = ConcurrencyController::new(4);
        assert_eq!(
            ctrl.try_acquire("coder-1", &["src/main.rs".to_owned()], false),
            AcquireOutcome::Acquired
        );
        let conflict = ctrl.try_acquire("coder-2", &["src/main.rs".to_owned()], false);
        match conflict {
            AcquireOutcome::Conflict {
                kind: ConflictKind::FileLock,
                detail,
            } => assert!(detail.contains("coder-1")),
            _ => panic!("expected file conflict"),
        }
        // Non-overlapping paths proceed.
        assert_eq!(
            ctrl.try_acquire("coder-3", &["src/other.rs".to_owned()], false),
            AcquireOutcome::Acquired
        );
        ctrl.release("coder-1");
        assert_eq!(
            ctrl.try_acquire("coder-2", &["src/main.rs".to_owned()], false),
            AcquireOutcome::Acquired
        );
    }

    #[test]
    fn browser_session_is_exclusive() {
        let mut ctrl = ConcurrencyController::new(4);
        assert_eq!(
            ctrl.try_acquire("browser-1", &[], true),
            AcquireOutcome::Acquired
        );
        assert_eq!(ctrl.browser_holder(), Some("browser-1"));
        let conflict = ctrl.try_acquire("browser-2", &[], true);
        assert!(matches!(
            conflict,
            AcquireOutcome::Conflict {
                kind: ConflictKind::BrowserBusy,
                ..
            }
        ));
        ctrl.release("browser-1");
        assert_eq!(ctrl.browser_holder(), None);
        assert_eq!(
            ctrl.try_acquire("browser-2", &[], true),
            AcquireOutcome::Acquired
        );
    }

    #[test]
    fn clamp_honours_absolute_max() {
        assert_eq!(ConcurrencyController::new(99).max_concurrent(), 4);
        assert_eq!(ConcurrencyController::new(0).max_concurrent(), 1);
    }
}
