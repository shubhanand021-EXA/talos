# Real computer-use mode (feature-gated)

> **Danger: this mode drives your real desktop.** The mock adapter is the
> default and is always safe. Real mode clicks, types, and scrolls for real,
> with no undo. Read this whole page before enabling it. Never enable it in
> CI, on shared machines, or while logged into anything sensitive.

## The three locks

Real input happens only when ALL of these hold. Removing any one disables
it completely.

1. **Build flag:** compile with `--features real-os-control`
   (e.g. `cargo run -p talos-desktop --features real-os-control`). Default
   builds physically cannot touch the OS (`DisabledRealComputer` only).
2. **Policy + consent:** the safety envelope must be enabled in the app,
   and every action needs a policy `Allow` verdict (UiAction mapping:
   reads Low, moves/scrolls Medium+evidence, everything else High and
   human-reviewed). High-risk actions go through the ApprovalSheet first.
3. **Process opt-in:** `TALOS_COMPUTER_REAL=1` must be set in the desktop
   process environment. A feature-enabled binary without it behaves exactly
   like the mock.

## Platform setup

| OS      | Accessibility                                                         | Capture    | Input                                                               | Notes                                                                                                                                                                                    |
| ------- | --------------------------------------------------------------------- | ---------- | ------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Windows | UI Automation (`uiautomation` 0.25)                                   | `xcap` 0.9 | `enigo` 0.6                                                         | No extra setup. Verified 2026-11-01.                                                                                                                                                     |
| macOS   | AXUIElement (`axuielement` 0.10)                                      | `xcap` 0.9 | `enigo` 0.6                                                         | Grant Accessibility permission: System Settings → Privacy & Security → Accessibility → allow the Talos binary. Without it every call fails closed. Requires a platform verification run. |
| Linux   | AT-SPI2 (`atspi` 0.30, needs a running at-spi bus: GNOME/KDE default) | `xcap` 0.9 | `enigo` 0.6 (X11; Wayland via libei where the compositor allows it) | Needs D-Bus session + system Wayland/X11 dev packages to build (`pkg-config`, compositor libs). Requires a platform verification run.                                                    |

Focus tracking uses `active-win-pos-rs` on all platforms (its Linux build
needs the same Wayland dev packages as above).

## What is blocked, always

- Password managers, keychains, credential stores — hard block.
- System settings / preferences — hard block.
- Payment, checkout, banking, wallet windows — hard block (ADR 0008
  tightened the ADR 0002 warning).
- Terminals, shells, consoles — allowed with a warning banner.
- Any application outside the session allowlist (when set) — `session_blocked`.
- Element confidence below 0.7 — refused (`low_confidence`).
- More than 30 live actions/minute — rate limited.
- Coordinates outside the viewport — refused.

## Running it

```sh
# 1. Build with the feature (never in CI).
cargo run -p talos-desktop --features real-os-control

# 2. In a *separate* shell, export the process opt-in before launching:
TALOS_COMPUTER_REAL=1 cargo run -p talos-desktop --features real-os-control
```

Then in the Computer page: enable the envelope, set the session allowlist
to the ONE application under test, dry-run first, approve high-risk
actions, and watch the live view (thumbnails update after each action —
nothing streams).

## The vision loop (what happens per action)

1. Capture the frame (`sha256:` hash) + read the focused window.
2. Check the session scope (allowlist, budgets) — blocked apps deny here.
3. Resolve the target (accessibility hit-test, else coordinate mapping) and
   gate on confidence.
4. Record the pointer position (kill-switch baseline) and inject.
5. If the pointer moved without Talos causing it (>8px drift): abort with
   `manual_intervention` — a human took over.
6. Re-check focus: vanished focus or a move into a blocked app sets
   `focusLost` and emits `computerFocusLost`.
7. Capture again and compare hashes. Click-like actions that change nothing
   set `verificationFailed` and emit `actionVerificationFailed`.
8. Store before/after frames content-addressed, append the evidence receipt,
   count the session budget.

## Emergency stop

- Click **Take Control** (or Emergency stop): releases all mouse/keyboard
  hooks immediately, latches the stop, cancels in-flight queue tasks, and
  the desktop backend logs
  `Emergency stop activated at [timestamp], [N] actions were in progress`.
- If you move the mouse yourself mid-injection, the kill switch fires first.
- If focus jumps to a blocked app, the action is flagged for human review.
- Reset is explicit (Reset stop) and never resumes canceled tasks.

## Disabling

Unset `TALOS_COMPUTER_REAL`, or rebuild without the feature. The envelope
`enabled` flag defaults to off on every launch; flipping it off mid-session
blocks new live actions instantly (dry-runs still plan).

## Integration tests (local only)

```sh
# Safe: capture + element identification only, skips without the env var.
TALOS_REAL_TESTS=1 cargo test -p computer_use --features real-os-control --test real_driver
```

These tests never click or type and never run in CI (`#![cfg(feature =
"real-os-control")]` plus the env gate).
