# browser_use

Persistent browser profiles, the human-in-the-loop CAPTCHA/auth bridge, and
keychain cookie injection for the Talos browser agent.

```sh
cargo test -p browser_use
```

## Why this crate exists

In v0.3.0/v0.4.0 Playwright is instantiated fresh for every task: modern web
apps that need stateful cookies and session storage force a re-login per run,
and a Cloudflare/CAPTCHA wall fails the task outright. This crate is the Rust
authority for closing that gap:

- **Persistent profiles** (`profile.rs`): Gateway-managed `user_data_dir`
  roots, default `~/.talos/browser_profiles/<name>` (`TALOS_BROWSER_PROFILE_ROOT`
  override). `ensure_profile` / `list_profiles` / `clear_profile` back the
  `list_browser_profiles` / `clear_browser_profile` IPC and Gateway commands.
  Markers always record `passwords: "never"` — profiles persist cookies and
  storage, never saved passwords.
- **HITL CAPTCHA/auth bridge** (`hitl.rs`): when the Browser Agent detects a
  CAPTCHA or login wall (DOM heuristics in `detect_intervention`, or an
  explicit `browser.wait_for_human` tool call), execution PAUSES on a
  `HitlRegistry` gate. The Desktop UI shows the `Talos requires manual browser
intervention` modal over a foreground Playwright window (or the streamed CDP
  view); the user solves the challenge and clicks **Resume**, the Gateway
  resolves the gate, and the agent continues from the paused state. `Dismiss`
  routes the agent around the wall; silence times the tool call out
  (`hitl_timeout`).
- **Cookie/secret injection** (`secrets_inject.rs`): session cookies and auth
  headers resolve from the OS keychain (`talos_secrets`) at launch and seed
  the Playwright context, skipping manual login flows. Values only ever travel
  into the browser context — plans, receipts, events, and logs carry key
  _names_ and hashes via `InjectionPlan::redacted_summary`.

## Contracts

`BrowserProfile` (`{ name, userDataDir, persistent, createdAt }`) and
`HitlBrowserEvent` (`{ interventionId, taskId, kind, url?, reason, at }`,
`kind: captcha_detected | auth_required`) mirror `@talos/contracts`
`browser.ts` exactly. The Playwright worker (`packages/browser-use`) is the
execution counterpart: `profiles.ts` / `hitl.ts` implement the same roots
and heuristics in TypeScript.
