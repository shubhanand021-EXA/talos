# computer_use

Deterministic safety for the Talos desktop: policy-gated, evidence-producing,
dry-run-first interaction. The mock computer (virtual screen, seeded
elements, resolution reports, `sha256:` screen hashes) runs everywhere with
no OS calls; the real OS driver lives behind the explicit `real-os-control`
cargo feature and never touches CI or default builds.

```sh
cargo test -p computer_use
# default: mock only, no OS access

cargo test -p computer_use --features real-os-control
# + gated driver, vision, session, injection unit tests (still no OS calls)

TALOS_REAL_TESTS=1 cargo test -p computer_use --features real-os-control --test real_driver
# real screenshot capture + element identification on this machine only
```

## Modules

- `safety`: mandatory gate (emergency stop, enable flag, rate limit,
  viewport bounds, confidence threshold, dangerous zones — settings and
  password managers hard-blocked, payment/banking hard-blocked since ADR
  0008, terminals warn — session isolation via `SessionScope`).
- `session`: allow/block lists, per-task overrides, time + action budgets;
  password managers and system settings can never be allowlisted.
- `driver`: platform-neutral `AccessibilityDriver` trait (focused window,
  element tree, hit-test, text, focus).
- `platform::{windows,macos,linux,fallback}`: UI Automation / AXUIElement /
  AT-SPI2 backends plus a coordinate-mapping fallback (feature-gated).
- `vision`: screenshot records, `sha256:` hashing, frame comparison,
  `verify_action` (click-like actions must change the frame).
- `focus`: focus monitor + manual-pointer kill switch (8px tolerance).
- `injection`: process-wide injection guard with in-flight accounting for
  the emergency-stop log line.
- `artifacts`: content-addressed screenshot store (`sha256-<hex>.png`).
- `mock`: deterministic virtual screen (default builds, tests, UI demos).
- `real` / `real_impl`: disabled stub (default) vs observe → act → verify
  loop (feature + `TALOS_COMPUTER_REAL=1` + envelope + policy `Allow`).

See [ADR 0008](../docs/adr/0008-real-os-computer-driver.md) and
[COMPUTER_USE_REAL.md](../docs/COMPUTER_USE_REAL.md) (setup guides, risks,
enable/disable, emergency-stop procedures).
