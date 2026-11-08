//! Human-in-the-loop CAPTCHA/auth bridge.
//!
//! Flow: the Browser Agent detects a CAPTCHA or login wall (DOM heuristics in
//! [`detect_intervention`] or an explicit `browser.wait_for_human` tool call)
//! → execution PAUSES on a [`HitlRegistry`] gate → a `captcha_detected` /
//! `auth_required` event reaches the Desktop UI → the UI spawns a foreground
//! Playwright window (or streams the CDP view) for the user to solve the
//! challenge or log in → the user clicks Resume → the Gateway resolves the
//! gate → the agent continues from the paused state.
//!
//! Events and receipts carry task linkage, URLs, and reasons only — never
//! page content, credentials, or challenge images.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

/// Why the browser agent paused for a human.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HitlKind {
    /// CAPTCHA / bot-wall challenge (Cloudflare, reCAPTCHA, Turnstile, …).
    CaptchaDetected,
    /// Login wall (password field, SSO redirect, session expiry).
    AuthRequired,
}

impl HitlKind {
    /// Wire value (`captcha_detected` | `auth_required`).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CaptchaDetected => "captcha_detected",
            Self::AuthRequired => "auth_required",
        }
    }
}

/// Event emitted to the Desktop UI when the agent pauses for a human.
///
/// Mirrors `@talos/contracts` `HitlBrowserEventSchema`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HitlBrowserEvent {
    /// Gate id the UI resolves (`resume` / `dismiss`).
    pub intervention_id: String,
    /// Owning task (ledger linkage).
    pub task_id: String,
    /// `captcha_detected` | `auth_required`.
    pub kind: HitlKind,
    /// Paused page URL (when known).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Short, content-free reason (e.g. `cloudflare-challenge`).
    pub reason: String,
    /// RFC 3339 emission time.
    pub at: String,
}

/// How a paused gate was released.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitlOutcome {
    /// The human solved the challenge / logged in; continue execution.
    Resumed,
    /// The human dismissed the request; the agent must route around or fail.
    Dismissed,
    /// Nobody answered before the wait budget; the tool call times out.
    TimedOut,
}

/// One pending human-intervention request.
struct PendingRequest {
    event: HitlBrowserEvent,
    release: Option<tokio::sync::oneshot::Sender<HitlOutcome>>,
}

/// Callback invoked synchronously on every new pause request (UI event fan-out).
pub type HitlListener = std::sync::Arc<dyn Fn(HitlBrowserEvent) + Send + Sync>;

/// Pause/resume gate registry for browser HITL interventions.
///
/// Production wiring: the tool executor registers a request and awaits the
/// gate; the Desktop `resume_browser_hitl` command (or the Gateway forward)
/// resolves it. Hosts fan events out by installing a [`HitlListener`]:
/// the desktop shell emits the `browserHitlRequired` Tauri event, the
/// gateway daemon broadcasts `CaptchaDetected`/`AuthRequired`.
#[derive(Default)]
pub struct HitlRegistry {
    pending: Mutex<HashMap<String, PendingRequest>>,
    listener: Mutex<Option<HitlListener>>,
}

impl std::fmt::Debug for HitlRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let pending = self.pending.lock().map(|guard| guard.len()).unwrap_or(0);
        let has_listener = self
            .listener
            .lock()
            .map(|guard| guard.is_some())
            .unwrap_or(false);
        f.debug_struct("HitlRegistry")
            .field("pending", &pending)
            .field("has_listener", &has_listener)
            .finish()
    }
}

impl HitlRegistry {
    /// Create an empty registry (tests; production uses [`global_registry`]).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Install the pause-request fan-out (host event emission). Replaces any
    /// previous listener. The callback must be fast and non-blocking: it runs
    /// on the pausing executor's thread at registration time.
    pub fn set_listener(&self, listener: HitlListener) {
        *self.listener.lock().expect("hitl mutex") = Some(listener);
    }

    /// Register a pause request; returns the UI event plus a waiter.
    ///
    /// The waiter resolves when [`HitlRegistry::resolve`] fires, or with
    /// [`HitlOutcome::TimedOut`] after `wait_timeout`.
    pub fn request(
        &self,
        task_id: &str,
        kind: HitlKind,
        url: Option<String>,
        reason: &str,
    ) -> (HitlBrowserEvent, HitlWaiter) {
        let intervention_id = format!("hitl-{}", uuid::Uuid::new_v4());
        let event = HitlBrowserEvent {
            intervention_id: intervention_id.clone(),
            task_id: task_id.to_owned(),
            kind,
            url,
            reason: reason.chars().take(256).collect(),
            at: talos_common::now_iso(),
        };
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.pending.lock().expect("hitl mutex").insert(
            intervention_id.clone(),
            PendingRequest {
                event: event.clone(),
                release: Some(tx),
            },
        );
        // Fan out to the host UI immediately: the agent is about to block,
        // so the modal must render from this registration, not from the
        // eventual resume.
        if let Some(listener) = self.listener.lock().expect("hitl mutex").clone() {
            listener(event.clone());
        }
        (event, HitlWaiter { rx })
    }

    /// Resolve a pending gate (`true` when the id was pending).
    pub fn resolve(&self, intervention_id: &str, outcome: HitlOutcome) -> bool {
        let mut pending = self.pending.lock().expect("hitl mutex");
        let Some(request) = pending.remove(intervention_id) else {
            return false;
        };
        if let Some(release) = request.release {
            let _ = release.send(outcome);
        }
        true
    }

    /// Drop a gate without a verdict (the waiter already moved on after a
    /// timeout). Returns `true` when an orphaned entry was removed, so timed
    /// out tool calls never leak stale modal entries.
    pub fn abandon(&self, intervention_id: &str) -> bool {
        self.pending
            .lock()
            .expect("hitl mutex")
            .remove(intervention_id)
            .is_some()
    }

    /// Snapshot of pending UI events (the modal queue).
    #[must_use]
    pub fn pending(&self) -> Vec<HitlBrowserEvent> {
        self.pending
            .lock()
            .expect("hitl mutex")
            .values()
            .map(|request| request.event.clone())
            .collect()
    }

    /// Number of gates currently paused.
    #[must_use]
    pub fn len(&self) -> usize {
        self.pending.lock().expect("hitl mutex").len()
    }

    /// True when no gate is paused.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Awaiting end of a pause request (the agent side of the gate).
#[derive(Debug)]
pub struct HitlWaiter {
    rx: tokio::sync::oneshot::Receiver<HitlOutcome>,
}

impl HitlWaiter {
    /// Wait for the human verdict, timing out after `wait_timeout`.
    pub async fn wait(self, wait_timeout: std::time::Duration) -> HitlOutcome {
        match tokio::time::timeout(wait_timeout, self.rx).await {
            Ok(Ok(outcome)) => outcome,
            Ok(Err(_)) => HitlOutcome::Dismissed,
            Err(_) => HitlOutcome::TimedOut,
        }
    }
}

/// Process-global registry shared by the pausing executor and the resolving
/// UI/Gateway command paths.
pub fn global_registry() -> Arc<HitlRegistry> {
    static GLOBAL: OnceLock<Arc<HitlRegistry>> = OnceLock::new();
    GLOBAL.get_or_init(|| Arc::new(HitlRegistry::new())).clone()
}

/// Heuristic CAPTCHA / login-wall detection over page signals.
///
/// Returns the intervention kind plus a short content-free reason token.
/// Pure function (no I/O): unit-tested against representative markers.
/// Never returns page content — only stable reason tokens.
pub fn detect_intervention(
    url: Option<&str>,
    title: Option<&str>,
    html: Option<&str>,
) -> Option<(HitlKind, &'static str)> {
    let haystack = format!(
        "{} {} {}",
        url.unwrap_or_default(),
        title.unwrap_or_default(),
        html.unwrap_or_default()
    )
    .to_ascii_lowercase();
    // Truncate the scan: markers live in the first kilobytes; a bounded scan
    // keeps adversarial megabyte pages from stalling the agent loop.
    let scan: &str = &haystack[..haystack.len().min(32_768)];
    // CAPTCHA / bot-wall markers first (a login form behind Cloudflare is a
    // CAPTCHA first, an auth wall second).
    for (marker, reason) in [
        ("cf-challenge", "cloudflare-challenge"),
        ("cloudflare", "cloudflare-challenge"),
        ("__cf_chl", "cloudflare-challenge"),
        ("turnstile", "turnstile-challenge"),
        ("recaptcha", "recaptcha-challenge"),
        ("g-recaptcha", "recaptcha-challenge"),
        ("hcaptcha", "hcaptcha-challenge"),
        ("h-captcha", "hcaptcha-challenge"),
        ("captcha", "captcha-challenge"),
        ("verify you are human", "human-verification"),
        ("verify you're human", "human-verification"),
        ("are you a robot", "human-verification"),
        ("datadome", "datadome-challenge"),
        ("perimeterx", "perimeterx-challenge"),
        ("kasada", "kasada-challenge"),
    ] {
        if scan.contains(marker) {
            return Some((HitlKind::CaptchaDetected, reason));
        }
    }
    for (marker, reason) in [
        ("input[type=password", "password-field"),
        ("type=\"password\"", "password-field"),
        ("sso", "sso-redirect"),
        ("single sign-on", "sso-redirect"),
        ("sign in to continue", "login-wall"),
        ("log in to continue", "login-wall"),
        ("session expired", "session-expired"),
        ("please log in", "login-wall"),
    ] {
        if scan.contains(marker) {
            return Some((HitlKind::AuthRequired, reason));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_captcha_markers() {
        let (kind, reason) =
            detect_intervention(None, Some("Just a moment..."), Some("cf-challenge-run"))
                .expect("cloudflare");
        assert_eq!(kind, HitlKind::CaptchaDetected);
        assert_eq!(reason, "cloudflare-challenge");
        let (kind, _) = detect_intervention(None, None, Some("<div class=\"g-recaptcha\"></div>"))
            .expect("recaptcha");
        assert_eq!(kind, HitlKind::CaptchaDetected);
    }

    #[test]
    fn detects_auth_walls() {
        let (kind, reason) = detect_intervention(
            Some("https://example.com/login"),
            None,
            Some("<input type=\"password\" name=\"pw\">"),
        )
        .expect("password");
        assert_eq!(kind, HitlKind::AuthRequired);
        assert_eq!(reason, "password-field");
    }

    #[test]
    fn captcha_wins_over_login_form() {
        let (kind, _) = detect_intervention(
            None,
            Some("Just a moment"),
            Some("cloudflare <input type=\"password\">"),
        )
        .expect("both");
        assert_eq!(kind, HitlKind::CaptchaDetected);
    }

    #[test]
    fn clean_pages_pass_through() {
        assert_eq!(
            detect_intervention(
                Some("https://example.com"),
                Some("Example"),
                Some("<h1>hi</h1>")
            ),
            None
        );
        assert_eq!(detect_intervention(None, None, None), None);
    }

    #[test]
    fn reason_tokens_carry_no_content() {
        let long = format!("captcha {}", "x".repeat(100_000));
        let (_, reason) = detect_intervention(None, None, Some(&long)).expect("hit");
        assert!(reason.len() < 64);
    }

    /// Mock CAPTCHA flow: execution pauses, waits for the resume signal,
    /// then continues from the paused state.
    #[tokio::test]
    async fn mock_captcha_flow_pauses_and_resumes() {
        let registry = HitlRegistry::new();
        let (event, waiter) = registry.request(
            "task-1",
            HitlKind::CaptchaDetected,
            None,
            "captcha-challenge",
        );
        assert_eq!(event.kind, HitlKind::CaptchaDetected);
        assert_eq!(registry.len(), 1);
        assert_eq!(registry.pending().len(), 1);
        // The agent side parks on the gate in the background (paused).
        let parked = tokio::spawn(waiter.wait(std::time::Duration::from_secs(5)));
        // Give the waiter a tick to park, then prove it is still paused.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(!parked.is_finished(), "execution must stay paused");
        // The human clicks Resume in the Desktop UI → Gateway resolves.
        assert!(registry.resolve(&event.intervention_id, HitlOutcome::Resumed));
        // Execution continues from the paused state.
        assert_eq!(
            parked.await.expect("join"),
            HitlOutcome::Resumed,
            "agent continues after resume"
        );
        assert!(registry.is_empty());
    }

    #[tokio::test]
    async fn gate_times_out_without_human() {
        let registry = HitlRegistry::new();
        let (event, waiter) =
            registry.request("task-2", HitlKind::AuthRequired, None, "login-wall");
        let outcome = waiter.wait(std::time::Duration::from_millis(20)).await;
        assert_eq!(outcome, HitlOutcome::TimedOut);
        // The tool call moved on: the orphaned gate is abandoned so no stale
        // modal entry lingers; a late human verdict finds nothing.
        assert!(registry.abandon(&event.intervention_id));
        assert!(!registry.resolve(&event.intervention_id, HitlOutcome::Resumed));
    }

    #[test]
    fn unknown_intervention_resolve_is_false() {
        assert!(!HitlRegistry::new().resolve("hitl-nope", HitlOutcome::Resumed));
    }

    #[test]
    fn listener_fires_on_registration() {
        use std::sync::{Arc, Mutex as StdMutex};
        let seen: Arc<StdMutex<Vec<HitlBrowserEvent>>> = Arc::new(StdMutex::new(Vec::new()));
        let seen_clone = Arc::clone(&seen);
        let registry = HitlRegistry::new();
        registry.set_listener(Arc::new(move |event| {
            seen_clone.lock().expect("seen").push(event);
        }));
        let (event, _waiter) = registry.request(
            "task-9",
            HitlKind::CaptchaDetected,
            None,
            "captcha-challenge",
        );
        let fired = seen.lock().expect("seen");
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].intervention_id, event.intervention_id);
        assert_eq!(fired[0].kind, HitlKind::CaptchaDetected);
    }

    #[test]
    fn event_shape_is_camel_case() {
        let event = HitlBrowserEvent {
            intervention_id: "hitl-1".to_owned(),
            task_id: "task-1".to_owned(),
            kind: HitlKind::AuthRequired,
            url: Some("https://example.com/login".to_owned()),
            reason: "login-wall".to_owned(),
            at: "2026-11-08T00:00:00Z".to_owned(),
        };
        let raw = serde_json::to_value(&event).expect("json");
        assert_eq!(raw["interventionId"], "hitl-1");
        assert_eq!(raw["kind"], "auth_required");
        assert!(raw.get("intervention_id").is_none());
    }
}
