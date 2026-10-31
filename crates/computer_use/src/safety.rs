//! Safety envelope: the mandatory gate every computer action passes.
//!
//! Rules (checked in order, first violation wins):
//! 1. Emergency stop latched → deny everything (dry-run included).
//! 2. Global enable flag off → deny live execution (dry-run still plans).
//! 3. Rate limit exceeded → deny live execution.
//! 4. Coordinates outside the viewport → deny.
//! 5. Blocking dangerous zone (system settings, password managers) → deny.
//! 6. Confidence below threshold → deny.
//!
//! Warning zones (terminals, payment flows) do not deny; they are reported
//! on the checked action so the UI can surface them.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use talos_common::{Result, TalosError};

use crate::model::{DangerZoneKind, SafetyConfig, SafetyViolation};

fn violation(code: &str, message: String) -> SafetyViolation {
    SafetyViolation {
        code: code.to_owned(),
        message,
    }
}

/// Result of a safety check: the checked action or a structured violation.
pub type SafetyResult = std::result::Result<CheckedAction, SafetyViolation>;

/// Input to a safety check (built by the tool executor from validated params).
#[derive(Debug, Clone, Default)]
pub struct SafetyInput {
    /// X coordinate, if the action targets a point.
    pub x: Option<i64>,
    /// Y coordinate, if the action targets a point.
    pub y: Option<i64>,
    /// Drag end X, if applicable.
    pub x2: Option<i64>,
    /// Drag end Y, if applicable.
    pub y2: Option<i64>,
    /// Element selector, if applicable.
    pub selector: Option<String>,
    /// Application hint (dangerous-zone matching).
    pub app_hint: Option<String>,
    /// Window title hint (dangerous-zone matching).
    pub window_hint: Option<String>,
    /// Caller-declared high risk.
    pub high_risk: bool,
    /// Caller-declared high-risk category.
    pub high_risk_kind: Option<String>,
    /// Dry-run mode (no mutation even when allowed).
    pub dry_run: bool,
}

/// A validated action ready for the adapter, with warnings attached.
#[derive(Debug, Clone, Default)]
pub struct CheckedAction {
    /// Non-blocking zone matches (terminal / payment-flow warnings).
    pub warnings: Vec<String>,
    /// Whether the caller flagged high risk.
    pub high_risk: bool,
    /// High-risk category, if any.
    pub high_risk_kind: Option<String>,
}

/// Cloneable handle to the shared envelope state (owned by the desktop layer).
#[derive(Debug, Clone)]
pub struct SafetyHandle {
    inner: Arc<SafetyInner>,
}

#[derive(Debug)]
struct SafetyInner {
    enabled: AtomicBool,
    stopped: AtomicBool,
    config: EnvelopeConfig,
    hits: tokio::sync::Mutex<VecDeque<Instant>>,
}

#[derive(Debug)]
struct EnvelopeConfig {
    config: SafetyConfig,
}

impl SafetyHandle {
    /// Build an envelope from explicit configuration.
    #[must_use]
    pub fn new(config: SafetyConfig) -> Self {
        Self {
            inner: Arc::new(SafetyInner {
                enabled: AtomicBool::new(config.enabled),
                stopped: AtomicBool::new(false),
                config: EnvelopeConfig { config },
                hits: tokio::sync::Mutex::new(VecDeque::new()),
            }),
        }
    }

    /// Current configuration snapshot.
    #[must_use]
    pub fn config(&self) -> SafetyConfig {
        self.inner.config.config.clone()
    }

    /// Global master switch (default false).
    pub fn set_enabled(&self, enabled: bool) {
        self.inner.enabled.store(enabled, Ordering::SeqCst);
    }

    /// Whether live execution is globally allowed.
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.inner.enabled.load(Ordering::SeqCst)
    }

    /// Latch the emergency stop: blocks every new computer action.
    pub fn engage_stop(&self) {
        self.inner.stopped.store(true, Ordering::SeqCst);
    }

    /// Clear the emergency stop (explicit reset action).
    pub fn clear_stop(&self) {
        self.inner.stopped.store(false, Ordering::SeqCst);
    }

    /// Whether the emergency stop is latched.
    #[must_use]
    pub fn is_stopped(&self) -> bool {
        self.inner.stopped.load(Ordering::SeqCst)
    }

    /// Snapshot for status panels and events.
    pub async fn status_snapshot(&self) -> SafetyStatus {
        let actions_this_minute = self.prune_and_count().await;
        SafetyStatus {
            enabled: self.is_enabled(),
            dry_run_default: self.inner.config.config.dry_run_default,
            emergency_stopped: self.is_stopped(),
            actions_this_minute,
            max_actions_per_minute: self.inner.config.config.max_actions_per_minute,
            viewport_width: self.inner.config.config.viewport_width,
            viewport_height: self.inner.config.config.viewport_height,
            min_confidence: self.inner.config.config.min_confidence,
        }
    }

    /// Consume one rate-limit slot for a live action.
    async fn try_acquire(&self) -> Result<()> {
        let count = self.prune_and_count().await;
        if count >= self.inner.config.config.max_actions_per_minute as usize {
            return Err(TalosError::Storage(format!(
                "rate limit: {count} actions in the last 60s (max {})",
                self.inner.config.config.max_actions_per_minute
            )));
        }
        let mut hits = self.inner.hits.lock().await;
        hits.push_back(Instant::now());
        Ok(())
    }

    /// Prune entries older than 60s and return the remaining count.
    async fn prune_and_count(&self) -> usize {
        let mut hits = self.inner.hits.lock().await;
        let cutoff = Instant::now() - Duration::from_secs(60);
        while hits.front().is_some_and(|front| *front < cutoff) {
            hits.pop_front();
        }
        hits.len()
    }

    /// Check one action against the envelope (no mutation, no policy call).
    pub async fn check_action(&self, input: &SafetyInput) -> SafetyResult {
        // 1. Emergency stop blocks everything, including dry-runs.
        if self.is_stopped() {
            return Err(violation(
                "emergency_stop",
                "emergency stop latched: clear it explicitly before any computer action".to_owned(),
            ));
        }
        // 2. Global enable flag gates live execution only.
        if !input.dry_run && !self.is_enabled() {
            return Err(violation(
                "computer_disabled",
                "computer-use is disabled by default; enable it explicitly first".to_owned(),
            ));
        }
        // 4. Coordinate bounds (viewport limits).
        let viewport = (
            self.inner.config.config.viewport_width,
            self.inner.config.config.viewport_height,
        );
        for (name, value) in [
            ("x", input.x),
            ("y", input.y),
            ("x2", input.x2),
            ("y2", input.y2),
        ] {
            if let Some(v) = value {
                let max = if name == "x" || name == "x2" {
                    viewport.0
                } else {
                    viewport.1
                };
                if !(0..=max).contains(&v) {
                    return Err(violation(
                        "out_of_bounds",
                        format!("{name}={v} outside viewport 0..={max}"),
                    ));
                }
            }
        }
        // 5. Dangerous zones by app/window hint.
        let haystack = format!(
            "{} {}",
            input.app_hint.as_deref().unwrap_or_default(),
            input.window_hint.as_deref().unwrap_or_default()
        )
        .to_lowercase();
        let mut warnings = Vec::new();
        for zone in &self.inner.config.config.danger_zones {
            if !haystack.contains(&zone.pattern.to_lowercase()) {
                continue;
            }
            if zone.blocking {
                let kind = match zone.kind {
                    DangerZoneKind::SystemSettings => "system settings",
                    DangerZoneKind::PasswordManager => "password manager",
                    _ => "blocked zone",
                };
                return Err(violation(
                    "dangerous_zone",
                    format!(
                        "{kind} target blocked (matched {:?} on {:?})",
                        zone.pattern,
                        input.app_hint.as_deref().unwrap_or_default()
                    ),
                ));
            }
            let kind = match zone.kind {
                DangerZoneKind::Terminal => "terminal",
                DangerZoneKind::PaymentFlow => "payment flow",
                _ => "sensitive area",
            };
            warnings.push(format!(
                "{kind} warning: matched {:?}; proceed with care",
                zone.pattern
            ));
        }
        // 3. Rate limit (live actions only; dry-runs are free to plan).
        if !input.dry_run {
            self.try_acquire().await.map_err(|err| {
                violation(
                    "rate_limited",
                    format!("{err} (slow down or raise max_actions_per_minute)"),
                )
            })?;
        }
        Ok(CheckedAction {
            warnings,
            high_risk: input.high_risk,
            high_risk_kind: input.high_risk_kind.clone(),
        })
    }
}

/// Status snapshot for UI panels and safety events.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SafetyStatus {
    /// Global master switch.
    pub enabled: bool,
    /// Dry-run is the default mode.
    pub dry_run_default: bool,
    /// Emergency stop latched.
    pub emergency_stopped: bool,
    /// Live actions in the last rolling 60s.
    pub actions_this_minute: usize,
    /// Rolling rate cap.
    pub max_actions_per_minute: u32,
    /// Virtual screen width.
    pub viewport_width: i64,
    /// Virtual screen height.
    pub viewport_height: i64,
    /// Minimum element confidence.
    pub min_confidence: f64,
}

/// Convenience alias: the envelope handle shared with the tool executor.
pub type SafetyEnvelope = SafetyHandle;

impl Default for SafetyHandle {
    fn default() -> Self {
        Self::new(SafetyConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope() -> SafetyHandle {
        SafetyHandle::new(SafetyConfig::default())
    }

    fn live_input() -> SafetyInput {
        SafetyInput {
            x: Some(100),
            y: Some(200),
            dry_run: false,
            ..SafetyInput::default()
        }
    }

    #[tokio::test]
    async fn emergency_stop_blocks_everything_including_dry_run() {
        let envelope = envelope();
        envelope.engage_stop();
        assert!(envelope.is_stopped());
        let denied = envelope
            .check_action(&SafetyInput {
                dry_run: true,
                ..live_input()
            })
            .await;
        let err = denied.expect_err("stopped envelope must deny dry-run");
        assert_eq!(err.code, "emergency_stop");
        let denied_live = envelope.check_action(&live_input()).await;
        assert_eq!(
            denied_live
                .expect_err("stopped envelope must deny live")
                .code,
            "emergency_stop"
        );
        envelope.clear_stop();
        assert!(!envelope.is_stopped());
    }

    #[tokio::test]
    async fn disabled_flag_blocks_live_but_allows_dry_run() {
        let envelope = envelope();
        assert!(!envelope.is_enabled());
        let live = envelope.check_action(&live_input()).await;
        assert_eq!(
            live.expect_err("disabled envelope must deny live").code,
            "computer_disabled"
        );
        let dry = envelope
            .check_action(&SafetyInput {
                dry_run: true,
                ..live_input()
            })
            .await;
        assert!(dry.is_ok());
    }

    #[tokio::test]
    async fn bounds_and_zones_are_enforced() {
        let envelope = envelope();
        let out = envelope
            .check_action(&SafetyInput {
                x: Some(9999),
                y: Some(10),
                dry_run: true,
                ..SafetyInput::default()
            })
            .await;
        assert_eq!(
            out.expect_err("out-of-bounds must deny").code,
            "out_of_bounds"
        );
        let blocked = envelope
            .check_action(&SafetyInput {
                app_hint: Some("System Settings".to_owned()),
                dry_run: true,
                ..SafetyInput::default()
            })
            .await;
        assert_eq!(
            blocked.expect_err("settings must deny").code,
            "dangerous_zone"
        );
        let warned = envelope
            .check_action(&SafetyInput {
                app_hint: Some("Terminal".to_owned()),
                dry_run: true,
                ..SafetyInput::default()
            })
            .await
            .expect("terminal is warn-only");
        assert_eq!(warned.warnings.len(), 1);
    }

    #[tokio::test]
    async fn rate_limit_caps_live_actions() {
        let config = SafetyConfig {
            enabled: true,
            max_actions_per_minute: 2,
            ..SafetyConfig::default()
        };
        let envelope = SafetyHandle::new(config);
        for _ in 0..2 {
            envelope
                .check_action(&SafetyInput {
                    dry_run: false,
                    x: Some(10),
                    y: Some(10),
                    ..SafetyInput::default()
                })
                .await
                .expect("within budget");
        }
        let limited = envelope
            .check_action(&SafetyInput {
                dry_run: false,
                x: Some(10),
                y: Some(10),
                ..SafetyInput::default()
            })
            .await;
        assert_eq!(
            limited.expect_err("third action must be limited").code,
            "rate_limited"
        );
        // Dry-runs never consume budget.
        envelope
            .check_action(&SafetyInput {
                dry_run: true,
                x: Some(10),
                y: Some(10),
                ..SafetyInput::default()
            })
            .await
            .expect("dry-run bypasses rate limit");
    }
}
