//! Gateway-managed persistent browser profiles for the Talos browser agent.
//!
//! The Playwright CDP worker is instantiated fresh for every task by default,
//! which breaks stateful web apps (cookies, session storage) and forces a
//! fresh login on every run. This crate is the Rust authority for the three
//! pieces that close that gap:
//!
//! - [`profile`]: dedicated `user_data_dir` roots managed by the Gateway
//!   (default `~/.talos/browser_profiles/<name>`), with list/clear commands.
//! - [`hitl`]: human-in-the-loop CAPTCHA/auth bridge. When the browser agent
//!   hits a Cloudflare/CAPTCHA wall or login gate, execution PAUSES on a
//!   registry gate until the Desktop UI resolves it (`Resume`/`Dismiss`).
//! - [`secrets_inject`]: cookie/auth-header injection plans whose values are
//!   resolved from the OS keychain ([`talos_secrets`]) at launch time, so
//!   manual login flows can be skipped without secrets ever touching the
//!   evidence ledger, IPC payloads, or logs (key *names* only outside the
//!   vault).
//!
//! Secret values never cross this crate's boundary except into the Playwright
//! launch context. Every error, event, and receipt carries names and hashes,
//! never values.

pub mod hitl;
pub mod profile;
pub mod secrets_inject;

pub use hitl::{
    detect_intervention, global_registry, HitlBrowserEvent, HitlKind, HitlOutcome, HitlRegistry,
};
pub use profile::{
    clear_profile, ensure_profile, gateway_profile_root, list_profiles, sanitize_profile_name,
    BrowserProfile, DEFAULT_PROFILE_NAME, PROFILE_ROOT_ENV,
};
pub use secrets_inject::{
    cookie_secret_name, header_secret_name, resolve_cookie_injections, resolve_header_injections,
    write_injection_file, CookieInjection, HeaderInjection, InjectionPlan,
};
