# ADR 0008: Real OS computer-use driver behind the feature gate

Date: 2026-11-01
Status: Accepted

## Context

ADR 0002 shipped the computer-use safety layer mock-only, with a
`compile_error!` gate reserving `real-os-control` for a future
human-approved adapter. The mission now calls for the real driver: OS
accessibility APIs + screenshot/vision verification, while keeping every
Batch 2 safety constraint (envelope, policy mapping, emergency stop,
isolation) and keeping CI green with zero OS interaction.

## Decision

Rust owns the whole real path; no new privileged shell, no TypeScript worker.

1. **Feature gate, three locks.** Real code compiles ONLY with the explicit
   `real-os-control` cargo feature (`computer_use`, forwarded by
   `tool_runtime` and `talos-desktop`; never enabled in CI or default
   builds). Runtime use additionally requires ALL of: the safety envelope
   enabled (user consent), a policy `Allow` verdict for the action (UiAction
   mapping unchanged), and `TALOS_COMPUTER_REAL=1` in the process environment
   (local-run opt-in so a feature build never touches the desktop by
   accident). The old `compile_error!` is replaced by the implementation;
   the _gate concept_ (default builds cannot touch the OS) is unchanged —
   default builds still link only `DisabledRealComputer`.
2. **Platform drivers behind one trait** (`AccessibilityDriver`:
   `focused_window`, `element_tree`, `element_at`, `text_of`,
   `focus_element`): Windows via `uiautomation` 0.25 (UI Automation),
   macOS via `axuielement` 0.10 (safe AXUIElement bindings, permission-gated),
   Linux via `atspi` 0.30 (AT-SPI2 registry + proxies), with a
   coordinate-mapping fallback everywhere. Element identification is
   structured first, coordinates second; confidence below the envelope
   threshold (0.7) refuses the action.
3. **One injection choke point.** All pointer/keyboard output goes through
   `enigo` 0.6 in `RealComputer::inject`, guarded by a process-wide
   `InjectionGuard` (RAII tokens, stop latch, in-flight accounting). Screens
   come from `xcap` 0.9; frames are `sha256:`-hashed, PNG-encoded with the
   `image` crate, stored content-addressed, and thumbnailed (base64 data
   URI, max 480px) for the UI.
4. **Observe → act → verify.** Capture before → resolve target → session
   scope + budgets → focus verify → inject with kill-switch baseline (manual
   pointer drift beyond 8px aborts with `manual_intervention`) → focus
   re-check (blocked-app or vanished focus sets `focusLost`) → capture after
   → hash compare (`verificationFailed` when a click-like action changes
   nothing). Failures surface as `actionVerificationFailed` /
   `computerFocusLost` Tauri events; receipts carry before/after hashes,
   artifacts, thumbnails, and element metadata.
5. **Session isolation in the envelope.** `SessionScope` (global allow/block
   lists, per-task overrides, max actions/age) lives in `SafetyHandle`;
   new IPC (`computer_session_scope`, `computer_set_session_scope`) backs the
   UI allowlist editor. Always-blocked floor (password managers, system
   settings) can never be allowlisted. Payment/banking windows now BLOCK
   (supersedes the ADR 0002 warn-only clause for payment flows; terminals
   stay warn-only).
6. **Emergency stop, upgraded.** Engaging the latch releases all mouse/
   keyboard hooks via the injection guard and logs
   `Emergency stop activated at [timestamp], [N] actions were in progress`.
   Reset is explicit and never resumes tasks.

## Consequences

- CI builds default features only: zero OS interaction, all existing tests
  unchanged (mock paths byte-identical). Feature builds are verified locally
  (`cargo check/test -p computer_use --features real-os-control`,
  gated integration tests in `tests/real_driver.rs` that skip without
  `TALOS_REAL_TESTS=1`).
- Windows driver is compiled and integration-tested on Windows (real
  screenshot capture, focused window, element hit-test verified 2026-11-01).
  macOS/Linux drivers are implemented against the documented crate APIs
  (`axuielement` 0.10, `atspi` 0.30) but require a platform verification run
  (macOS needs a Swift toolchain for `apple-cf`; Linux needs a D-Bus
  session + dev sysroot for cross-checks). See `docs/COMPUTER_USE_REAL.md`.
- New crates.io dependencies are optional and feature-gated; default builds
  and `Cargo.lock` for CI carry no new native requirements.
- The UI Computer page gains a live view (per-action thumbnails, never
  streamed), an action timeline with before/after hashes and verification
  outcomes, a Take Control button, and the allowlist editor.
