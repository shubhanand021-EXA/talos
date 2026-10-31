# ADR 0001: Browser-use architecture

Date: 2026-10-31
Status: accepted

## Context

Talos needs browser automation (navigate, click, type, scroll, select, extract, screenshot, DOM snapshot) that is policy-gated, evidence-producing, isolated, dry-run capable, and compensation-aware. CI must stay green without live browsers; local machines may optionally run real Chromium.

## Decision

Split ownership across two layers with a single source of truth:

1. **Rust remains authoritative** (`crates/tool_runtime`).
   Eight `browser.*` tools are registered in the existing `ToolRegistry` and
   executed through the existing gated `invoke()` path: validate → enabled-check
   → `ActionRequest` (capability `OpenBrowser`, risk from manifest, `highRisk`
   flag escalation) → audit → policy evaluate with live ledger context →
   deny/disabled short-circuit with structured errors → dry-run plan or live
   mock execution → receipt persisted as verified `tool_receipt` evidence.
   No new Tauri IPC commands: the generic `invoke_tool*` commands carry the new
   tools, so the desktop shell gains no broad shell access.

2. **TypeScript owns the worker and live adapter** (`packages/browser-use`).
   A deterministic `MockBrowserAdapter` (seeded fake DOM, FNV-1a + `sha256:`
   hashes, isolated per-task profile dirs) powers CI and the default path. An
   opt-in `LiveBrowserAdapter` dynamically imports Playwright only when
   `TALOS_BROWSER_LIVE=1` is set _and_ Playwright + Chromium are installed;
   otherwise it returns a structured unavailable reason. A local CLI worker
   (`talos-browser`) runs one JSON action deterministically with no display.

3. **Contracts are the single source of truth** (`packages/contracts/src/browser.ts`):
   `BrowserAction` (8), `BrowserTarget` (url/selector/frame/timeout),
   `BrowserReceipt` (actionId, status, domHash, screenshotHash,
   extractedText(+hash), timestamp, target URL, policy evaluation reference).

4. **Policy mapping**: read-only browser tools (`navigate/snapshot/extract/screenshot/scroll`)
   are Low risk and run directly; mutations (`click/type/select`) are Medium and
   require recorded evidence first (new `OpenBrowser`-Medium evidence gate in the
   broker); `highRisk` flags (form submit, purchase, settings change, upload /
   download, destructive) escalate to High + non-reversible, hitting the existing
   human-review rule. Default mode is dry-run.

5. **Isolation**: per-task profile dirs under the sandbox root (`browser-profiles/<task>`),
   never a personal profile; no passwords; no cookies persisted unless explicitly
   enabled; sessions close by removing the profile dir (persistent ones are kept).

## Consequences

- CI needs no browser, display, or network: all browser tests run mock-only.
- Live Chromium is a local opt-in (`pnpm add -D playwright`, download Chromium,
  `TALOS_BROWSER_LIVE=1`); absence is a structured disabled state, never silent.
- A future step can orchestrate the TS worker through the controlled
  `process.exec` tool; until then the Rust mock and TS mock share deterministic
  DOM semantics (same FNV-1a seed layout and `sha256:` hashes).
- High-risk web actions can never execute silently: they require explicit human
  approval or a dry-run-first evidence trail, and every receipt is ledgered.
