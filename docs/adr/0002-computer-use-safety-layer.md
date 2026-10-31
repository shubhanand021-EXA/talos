# ADR 0002: Computer-use safety layer

Date: 2026-10-31
Status: accepted

## Context

Talos needs computer-use automation (move, click, drag, type, hotkey, scroll, screenshot, locate) for the most dangerous capability class: simulated desktop input. Unlike files or browsers, there is no undo. CI must stay green with zero OS interaction; local runs must never touch the real desktop by accident.

## Decision

**Rust-first with clear Rust authority** (`crates/computer_use` + `crates/tool_runtime`):

1. **New crate, mock-only.** `crates/computer_use` owns the safety model
   (`SafetyEnvelope`: global enable flag off by default, dry-run default,
   30/min rate limit, 1440x900 viewport bounds, 0.7 confidence threshold,
   blocking vs warning dangerous zones, cooperative emergency-stop latch),
   the deterministic mock computer (virtual screen, seeded elements,
   resolution reports, `sha256:` screen hashes), and a disabled real-adapter
   stub (`RealComputerAdapter` trait + `DisabledRealComputer`; the
   `real-os-control` cargo feature fails the build by design until a future
   human-approved adapter lands with its own audit trail).
2. **No TypeScript worker.** Unlike browser-use, there is no live path to
   orchestrate yet (no Playwright equivalent is safe to invoke), so no
   `packages/computer-use` exists. The mock lives in Rust and the UI drives
   it through the existing generic `invoke_tool*` IPC — no new shell access.
3. **Contracts are the single source of truth** (`packages/contracts/src/computer.ts`):
   `ComputerAction` (9), `ComputerTarget` (coordinates/bounds, app/window
   hint, confidence), `ComputerActionRequest`, `ComputerReceipt`
   (screenHash, boundingBox, confidence, policy + evidence references).
4. **Policy mapping**: read-only tools (`capture_screen`, `locate_element`)
   are Low risk and run directly; `move_mouse`/`scroll` are Medium and require
   recorded evidence first (new `UiAction`-Medium evidence gate); all other
   mutations are High and non-reversible, hitting the existing human-review
   rule. A `highRisk` flag never downgrades; it only documents intent.
5. **Emergency stop** is a runtime latch: engaging it cancels in-flight queue
   tasks, blocks every new computer action including dry-runs, and emits
   `computerSafetyChanged`. Reset is explicit and never resumes tasks.
6. **Isolation**: per-task session dirs under the sandbox root
   (`computer-profiles/<task>`); never a personal profile; no passwords; no
   persisted cookies. Dangerous zones: system settings + password managers
   hard-block; terminals + payment flows warn.

## Consequences

- CI needs no display, OS hooks, or network: all computer tests run the mock.
- Real OS control is structurally impossible in this tree: disabled registry
  entries, disabled envelope, no OS calls in the mock, and a compile-error
  feature gate.
- High-risk and destructive computer actions are denied with an explicit
  human-review reason today; the approval surface and compensation wiring
  (`manual_review` plans) arrive with Prompt 06+ once a human-in-the-loop
  flow exists.
