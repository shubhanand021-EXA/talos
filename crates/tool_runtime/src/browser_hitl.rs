//! Executor-side human-in-the-loop bridge for `browser.wait_for_human`.
//!
//! The Browser Agent calls this tool when it hits a CAPTCHA or login wall.
//! Live execution registers a pause gate on the process-global
//! [`browser_use::HitlRegistry`], returns a `hitl_required` receipt shape so
//! the Desktop UI can render the intervention modal, and BLOCKS until the
//! human resolves the gate (`Resume` → success, `Dismiss` → failed, silence →
//! `hitl_timeout`). Dry-run only plans the pause (never registers a gate).
//!
//! Only linkage metadata (intervention id, task id, kind, URL, reason) ever
//! leaves this module toward receipts and events — never page content.

use crate::adapters::SandboxConfig;
use crate::model::{ToolErrorDetail, ToolStatus};

fn err(code: &str, message: String) -> ToolErrorDetail {
    ToolErrorDetail::new(code, message)
}

/// Planned pause effects (dry-run: no gate registered, no blocking).
#[must_use]
pub fn wait_plan(
    kind: &str,
    reason: &str,
    url: Option<&str>,
    timeout_ms: u64,
) -> serde_json::Value {
    serde_json::json!({
        "kind": kind,
        "reason": reason,
        "url": url,
        "timeoutMs": timeout_ms,
        "hitlRequired": true,
        "planned": true,
        "simulated": true,
    })
}

/// Live pause: register the gate, block for the human verdict, map it.
pub async fn wait_live(
    config: &SandboxConfig,
    task_id: &str,
    kind: browser_use::HitlKind,
    reason: &str,
    url: Option<&str>,
    timeout_ms: u64,
) -> (ToolStatus, serde_json::Value, Option<ToolErrorDetail>) {
    let profile_root = config
        .browser_profile_root
        .clone()
        .unwrap_or_else(browser_use::gateway_profile_root);
    let profile = browser_use::ensure_profile(&profile_root, browser_use::DEFAULT_PROFILE_NAME)
        .await
        .ok();
    let (event, waiter) =
        browser_use::global_registry().request(task_id, kind, url.map(str::to_owned), reason);
    let base = serde_json::json!({
        "interventionId": event.intervention_id,
        "kind": kind.as_str(),
        "reason": event.reason,
        "url": event.url,
        "taskId": task_id,
        "hitlRequired": true,
        "profileDir": profile.map(|p| p.user_data_dir),
        "simulated": false,
    });
    let outcome = waiter
        .wait(std::time::Duration::from_millis(timeout_ms))
        .await;
    match outcome {
        browser_use::HitlOutcome::Resumed => {
            let mut output = base;
            if let Some(object) = output.as_object_mut() {
                object.insert("resumed".to_owned(), serde_json::Value::Bool(true));
            }
            (ToolStatus::Success, output, None)
        }
        browser_use::HitlOutcome::Dismissed => (
            ToolStatus::Failed,
            base,
            Some(err(
                "hitl_dismissed",
                "human dismissed the browser intervention; route around or fail".to_owned(),
            )),
        ),
        browser_use::HitlOutcome::TimedOut => {
            // The agent moved on: drop the orphaned gate so the UI modal
            // queue never shows a stale intervention.
            browser_use::global_registry().abandon(&event.intervention_id);
            (
                ToolStatus::Failed,
                base,
                Some(err(
                    "hitl_timeout",
                    format!("no human verdict within {timeout_ms}ms"),
                )),
            )
        }
    }
}

/// Resolve one paused gate (Desktop `Resume` / `Dismiss` path).
#[must_use]
pub fn resolve_intervention(intervention_id: &str, outcome: browser_use::HitlOutcome) -> bool {
    browser_use::global_registry().resolve(intervention_id, outcome)
}

/// Pending intervention events for the UI modal queue (metadata only).
#[must_use]
pub fn pending_interventions() -> Vec<browser_use::HitlBrowserEvent> {
    browser_use::global_registry().pending()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_registers_no_gate() {
        let before = browser_use::global_registry().len();
        let planned = wait_plan("captcha_detected", "captcha-challenge", None, 60_000);
        assert_eq!(planned["hitlRequired"], true);
        assert_eq!(planned["planned"], true);
        assert_eq!(browser_use::global_registry().len(), before);
    }

    /// Full pause/resume proof through the executor-facing helper: the live
    /// call parks on the gate, the UI resolves it, the call continues.
    #[tokio::test]
    async fn live_call_pauses_and_continues_on_resume() {
        use std::sync::Arc;
        let dir = tempfile::tempdir().expect("tempdir");
        let config = Arc::new(SandboxConfig {
            root: dir.path().to_path_buf(),
            browser_profile_root: Some(dir.path().join("browser_profiles")),
            ..SandboxConfig::default()
        });
        let parked = tokio::spawn({
            let config = Arc::clone(&config);
            async move {
                wait_live(
                    &config,
                    "task-hitl-1",
                    browser_use::HitlKind::CaptchaDetected,
                    "captcha-challenge",
                    Some("https://example.com/protected"),
                    10_000,
                )
                .await
            }
        });
        // Wait until the gate is actually registered (paused).
        let intervention = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let pending = pending_interventions();
                if let Some(event) = pending.iter().find(|event| event.task_id == "task-hitl-1") {
                    return event.clone();
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("gate registers");
        assert_eq!(intervention.kind, browser_use::HitlKind::CaptchaDetected);
        assert!(!parked.is_finished(), "execution must stay paused");
        assert!(resolve_intervention(
            &intervention.intervention_id,
            browser_use::HitlOutcome::Resumed
        ));
        let (status, output, error) = parked.await.expect("join");
        assert_eq!(status, ToolStatus::Success);
        assert!(error.is_none());
        assert_eq!(output["resumed"], true);
        assert_eq!(output["hitlRequired"], true);
        assert!(pending_interventions()
            .iter()
            .all(|event| event.task_id != "task-hitl-1"));
    }

    #[tokio::test]
    async fn live_call_times_out_without_human() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = SandboxConfig {
            root: dir.path().to_path_buf(),
            browser_profile_root: Some(dir.path().join("browser_profiles")),
            ..SandboxConfig::default()
        };
        let (status, output, error) = wait_live(
            &config,
            "task-hitl-timeout",
            browser_use::HitlKind::AuthRequired,
            "login-wall",
            None,
            30,
        )
        .await;
        assert_eq!(status, ToolStatus::Failed);
        assert_eq!(
            error.map(|error| error.code),
            Some("hitl_timeout".to_owned())
        );
        assert_eq!(output["hitlRequired"], true);
    }
}
