//! Session isolation: which applications Talos may touch, and for how long.
//!
//! A [`SessionScope`] is the per-deployment allowlist/blocklist contract:
//! global allow/block lists plus optional per-task overrides. Blocked
//! applications always win (password managers and system settings can never
//! be allowlisted back in). Time and action budgets bound a session.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Application allowlist for one task (overrides the global lists).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppScope {
    /// Allowed application substrings (empty = inherit the global list).
    pub allowed_apps: Vec<String>,
    /// Extra blocked substrings for this task.
    pub blocked_apps: Vec<String>,
}

/// Session isolation scope: app allow/block lists, budgets, per-task rules.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionScope {
    /// Allowed application substrings (case-insensitive). Empty = any app
    /// except blocked ones.
    pub allowed_apps: Vec<String>,
    /// Blocked application substrings (case-insensitive); always wins.
    pub blocked_apps: Vec<String>,
    /// Per-task overrides, keyed by task id.
    pub per_task: HashMap<String, AppScope>,
    /// Maximum live actions per session (`None` = unbounded).
    pub max_actions: Option<u64>,
    /// Maximum session age in seconds (`None` = unbounded).
    pub max_age_secs: Option<u64>,
}

/// Applications that can never be allowlisted (hard-coded safety floor).
pub const ALWAYS_BLOCKED: &[&str] = &[
    "password",
    "keychain",
    "1password",
    "bitwarden",
    "keepass",
    "lastpass",
    "dashlane",
    "settings",
    "preferences",
    "system settings",
    "system preferences",
];

fn matches_any(haystack: &str, patterns: &[String]) -> bool {
    let lower = haystack.to_lowercase();
    patterns
        .iter()
        .any(|pattern| !pattern.trim().is_empty() && lower.contains(&pattern.to_lowercase()))
}

impl SessionScope {
    /// True when `app_name` may be automated for `task_id`.
    ///
    /// Order: always-blocked floor first, then the union of global and
    /// per-task block lists, then the allow lists (empty allow = allow all).
    #[must_use]
    pub fn app_allowed(&self, task_id: &str, app_name: &str) -> bool {
        if matches_any(
            app_name,
            &ALWAYS_BLOCKED
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
        ) {
            return false;
        }
        if matches_any(app_name, &self.blocked_apps) {
            return false;
        }
        if let Some(task_scope) = self.per_task.get(task_id) {
            if matches_any(app_name, &task_scope.blocked_apps) {
                return false;
            }
            if !task_scope.allowed_apps.is_empty() {
                // A per-task allowlist narrows the global one.
                return matches_any(app_name, &task_scope.allowed_apps);
            }
        }
        if self.allowed_apps.is_empty() {
            return true;
        }
        matches_any(app_name, &self.allowed_apps)
    }

    /// Human reason for the current verdict (for denials and audit trails).
    #[must_use]
    pub fn verdict_reason(&self, task_id: &str, app_name: &str) -> String {
        if self.app_allowed(task_id, app_name) {
            format!("session scope allows {app_name:?} for task {task_id}")
        } else {
            format!("session scope blocks {app_name:?} for task {task_id}")
        }
    }
}

/// Mutable per-task session state: age and action budget accounting.
#[derive(Debug)]
pub struct SessionState {
    /// When the session started.
    pub started_at: std::time::Instant,
    /// Live actions executed so far.
    pub actions: u64,
}

impl SessionState {
    /// Start a fresh session clock.
    #[must_use]
    pub fn start() -> Self {
        Self {
            started_at: std::time::Instant::now(),
            actions: 0,
        }
    }

    /// Enforce time and action budgets; counts this action on success.
    pub fn check_and_note(&mut self, scope: &SessionScope) -> Result<(), String> {
        if let Some(max_age) = scope.max_age_secs {
            if self.started_at.elapsed().as_secs() > max_age {
                return Err(format!("session expired after {max_age}s"));
            }
        }
        if let Some(max_actions) = scope.max_actions {
            if self.actions >= max_actions {
                return Err(format!("session action budget exhausted ({max_actions})"));
            }
        }
        self.actions += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scoped() -> SessionScope {
        SessionScope {
            allowed_apps: vec!["notes".to_owned(), "calculator".to_owned()],
            blocked_apps: vec!["browser".to_owned()],
            per_task: [(
                "task-1".to_owned(),
                AppScope {
                    allowed_apps: vec!["notes".to_owned()],
                    blocked_apps: Vec::new(),
                },
            )]
            .into_iter()
            .collect(),
            max_actions: Some(2),
            max_age_secs: Some(3600),
        }
    }

    #[test]
    fn always_blocked_wins_over_allowlists() {
        let scope = scoped();
        assert!(!scope.app_allowed("task-9", "1Password 8"));
        assert!(!scope.app_allowed("task-9", "System Settings"));
        assert!(!scope.app_allowed("task-9", "My Password Manager"));
    }

    #[test]
    fn task_override_narrows_the_global_list() {
        let scope = scoped();
        assert!(scope.app_allowed("task-1", "Notes"));
        assert!(!scope.app_allowed("task-1", "Calculator"));
        assert!(!scope.app_allowed("task-9", "browser-x"));
    }

    #[test]
    fn empty_allow_means_allow_except_blocked() {
        let scope = SessionScope::default();
        assert!(scope.app_allowed("t", "Notes"));
        assert!(!scope.app_allowed("t", "System Settings"));
    }

    #[test]
    fn action_budget_is_counted() {
        let mut state = SessionState::start();
        let scope = scoped();
        assert!(state.check_and_note(&scope).is_ok());
        assert!(state.check_and_note(&scope).is_ok());
        assert!(state.check_and_note(&scope).is_err());
    }
}
