//! Real-driver integration tests: feature-gated, never in CI.
//!
//! Run locally with a display session:
//! `cargo test -p computer_use --features real-os-control --test real_driver`
//!
//! Safe by default: capture + element identification only. Destructive input
//! (click/type) runs only with `TALOS_REAL_TESTS=write`, and even then only
//! against coordinates — never as part of an automated suite.

#![cfg(feature = "real-os-control")]

use computer_use::{AccessibilityDriver, PlatformDriver};

/// Skip unless explicitly opted in (prevents accidental desktop interaction).
fn opted_in() -> bool {
    std::env::var("TALOS_REAL_TESTS").as_deref() == Ok("1")
}

#[tokio::test]
async fn real_screenshot_capture_produces_hashed_frames() {
    if !opted_in() {
        eprintln!("skipping: set TALOS_REAL_TESTS=1 for real-driver tests");
        return;
    }
    let png = tokio::task::spawn_blocking(|| {
        let monitors = xcap::Monitor::all().expect("monitors");
        assert!(!monitors.is_empty(), "at least one monitor");
        let image = monitors[0].capture_image().expect("capture");
        assert!(image.width() > 0 && image.height() > 0);
        image.into_raw()
    })
    .await
    .expect("capture task joins");
    let hash = computer_use::vision::hash_bytes(&png);
    assert!(hash.starts_with("sha256:"));
}

#[tokio::test]
async fn real_focused_window_resolves() {
    if !opted_in() {
        eprintln!("skipping: set TALOS_REAL_TESTS=1 for real-driver tests");
        return;
    }
    let driver = PlatformDriver::system();
    let window = driver.focused_window().await.expect("focused window");
    assert!(!window.app_name.trim().is_empty(), "app name resolves");
}

#[tokio::test]
async fn real_element_at_point_resolves() {
    if !opted_in() {
        eprintln!("skipping: set TALOS_REAL_TESTS=1 for real-driver tests");
        return;
    }
    let driver = PlatformDriver::system();
    let element = driver.element_at(100, 100).await.expect("element");
    assert!(!element.role.trim().is_empty(), "role resolves");
    assert!(
        (0.0..=1.0).contains(&element.confidence),
        "confidence in range"
    );
}

#[tokio::test]
async fn real_execution_stays_off_without_env_opt_in() {
    // Safety property: without TALOS_COMPUTER_REAL=1 the router must decline
    // even in a feature-enabled binary (mock fallthrough).
    std::env::remove_var("TALOS_COMPUTER_REAL");
    let envelope = computer_use::SafetyHandle::default();
    let dir = tempfile::tempdir().expect("tempdir");
    let routed = computer_use::real::try_execute_live(
        "computer.click",
        "task-1",
        &serde_json::json!({ "x": 10, "y": 10 }),
        dir.path(),
        &envelope,
    )
    .await;
    assert!(routed.is_none(), "real mode must decline without opt-in");
}
