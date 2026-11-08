# @talos/browser-use

Talos browser-use foundation: isolated sessions, DOM-first safety, deterministic mock adapter, production live Chromium.

## Architecture

TypeScript owns the browser worker (`packages/browser-use`); Rust stays authoritative for policy, evidence, and compensation. The Rust tool framework (`crates/tool_runtime`) exposes nine actions (`browser.navigate/click/type/scroll/select/extract/screenshot/snapshot/session`) through the generic `invoke_tool*` IPC — no new Tauri commands, no broad shell access. With `TALOS_BROWSER_LIVE=1` the executor spawns `node packages/browser-use/dist/worker.js` per call (JSON on argv, receipt JSON on stdout, 30s kill timeout); otherwise the deterministic in-process mock runs (seeded FNV-1a DOM, `sha256:` hashes).

## Safety defaults

- Isolated profile per task under `<sandbox>/browser-profiles/<task>`; never a personal profile; no saved passwords; no cookies persisted unless explicitly enabled.
- Dry-run first: selector resolution + validation without page mutation.
- Read-only actions (`navigate/snapshot/extract/screenshot/scroll`) run Low risk; mutations (`click/type/select`) are evidence-gated Medium; `highRisk` flags (form submit, purchase, settings, upload/download, destructive) escalate to human review.
- Live Chromium is **disabled by default** and CI never touches the network.

## Live mode (opt-in, local only)

```sh
pnpm --filter @talos/browser-use run setup:chromium   # playwright install chromium
TALOS_BROWSER_LIVE=1 pnpm --filter @talos/browser-use run worker --live '{"action":"snapshot_dom","url":"https://example.com"}'
```
Live execution rules (enforced in worker AND Rust, so neither side can be bypassed):

- Activation requires `TALOS_BROWSER_LIVE=1`; without it (or without Playwright/Chromium) calls return `{available: false}` or mock receipts — never a silent pass.
- Launch: headless by default (`TALOS_BROWSER_HEADLESS=0` for headed debugging), isolated profile under `browser-profiles/<sanitized-task>`, args `--no-first-run --disable-default-apps --disable-extensions`, no personal profile reuse, no saved passwords, no cookies unless explicitly enabled per session.
- Every action resolves its selector first (found/visible/interactable report); unresolved selectors fail closed without touching the page.
- Mutations (`click`/`type`/`select`) default to dry-run; live runs need policy `Allow`, and `highRisk` actions need explicit `approved: true` (or they return `approval_required`).
- Password fields are never auto-filled: typing into one returns `auth_required` and emits `browserAuthRequired` for human handling.
- Navigation is restricted to `http(s)`; `file:`, `chrome:`, `about:`, `javascript:`, and `data:` URLs are rejected. `TALOS_BROWSER_ALLOWED_ORIGINS` (comma list) restricts origins further; `TALOS_BROWSER_SENSITIVE_DOMAINS` marks pages whose screenshots are hash-only (never stored).
- Screenshots with password fields present are masked before hashing; artifacts persist content-addressed (`artifacts/<sha256>.png`) only when < 5 MiB and not sensitive.
- Sessions: at most 1 warm session by default (configurable), 5-minute idle auto-close, explicit `session.launch|status|close` control, `session.close_all` on emergency stop.
- Worker protocol: single-shot JSON on argv/stdout, or `--serve` line-delimited JSON-RPC (`browser.*`, `session.*`, `ping`) with async events (`browserAuthRequired`, `browserSessionClosed`, `browserWorkerCrashed`). Per-call 30s timeout kills the child; crashes surface as structured `browser_worker_crashed` receipts and Tauri events.

## Persistent profiles (Gateway-managed)

By default every task gets a fresh isolated profile. For stateful web apps,
opt into a Gateway-managed persistent `user_data_dir`:

- Root: `~/.talos/browser_profiles` (`TALOS_BROWSER_PROFILE_ROOT` overrides;
  `TALOS_BROWSER_PROFILE` selects the name, `default` when unset).
- `TALOS_BROWSER_PERSISTENT=1` makes every launch reuse the persistent
  profile; per-call `"profile": "<name>"` (or `browser.session launch` with
  `profile`/`persistent`) selects one explicitly.
- `profile.list` / `profile.clear` (serve mode) and the
  `list_browser_profiles` / `clear_browser_profile` IPC + Gateway commands
  manage them. Markers record `passwords: "never"` — cookies and storage
  persist, saved passwords never do. The Rust authority is
  `crates/browser_use/src/profile.rs`.

## HITL CAPTCHA/auth bridge

When the live adapter detects a CAPTCHA or login wall (heuristics in
`src/hitl.ts`: Cloudflare/Turnstile/reCAPTCHA/hCaptcha markers win over login
forms; password fields, SSO, and session-expiry markers follow), `navigate`
fails with `captcha_detected` / `auth_required` and emits
`browserCaptchaDetected` / `browserAuthRequired`. The Browser Agent then calls
`browser.wait_for_human`, which PAUSES on a gate:

- Rust tool (`tool_runtime`): blocks until the Desktop UI resolves the gate
  (`browserHitlRequired` Tauri event → `HitlInterventionModal` → foreground
  Playwright window on the same `user_data_dir` → Resume/Dismiss) or the
  verdict budget (`timeoutMs`, 10s–10min) expires.
- Serve mode (`--serve`): `browser.wait_for_human` returns a `paused` receipt
  with the modal event immediately; the agent polls `browser.hitl_status`
  until `browser.resume_human` releases the gate.

## Cookie/secret injection

`browser.session launch` accepts `cookieSecrets` / `authHeaderSecrets`
(keychain key *names* like `browser.cookie.{profile}.{host}.{name}`). The
desktop shell resolves values from the OS keychain and stages a `0600`
`<profileDir>/injections.json` handoff the worker seeds into the fresh
context (`addCookies` / `setExtraHTTPHeaders`). Values never cross argv,
logs, receipts, or events — the Rust authority is
`crates/browser_use/src/secrets_inject.rs`.

## Worker CLI

```sh
pnpm --filter @talos/browser-use run build
node packages/browser-use/dist/worker.js '{"action":"snapshot_dom","url":"https://example.com"}'
echo '{"action":"extract_text","url":"https://example.com","maxChars":100}' | node packages/browser-use/dist/worker.js
```

Input is a single JSON action (`action`, `url`, `selector`, `text`, `direction`, `maxChars`, `highRisk`, `highRiskKind`, ...); output is a receipt-shaped JSON object. Deterministic, no display required.
