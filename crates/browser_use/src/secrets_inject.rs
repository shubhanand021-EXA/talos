//! Cookie/secret injection from the OS keychain.
//!
//! Manual login flows are the slow path: when the operator has already stored
//! session cookies or auth headers in the keychain (via [`talos_secrets`]),
//! the Browser Agent injects them into the Playwright context at launch and
//! skips the login UI entirely.
//!
//! Key rule: secret *values* are resolved here and handed to the browser
//! context only. Every plan, receipt, event, and log carries key *names*,
//! counts, and hashes — never values. [`InjectionPlan::redacted_summary`]
//! is the only shape that may leave this module toward the ledger.

use talos_common::{Result, TalosError};

fn err(msg: impl Into<String>) -> TalosError {
    TalosError::Storage(msg.into())
}

/// Canonical keychain key for one injected cookie:
/// `browser.cookie.{profile}.{host}.{name}`.
#[must_use]
pub fn cookie_secret_name(profile: &str, host: &str, cookie: &str) -> String {
    format!("browser.cookie.{profile}.{host}.{cookie}")
}

/// Canonical keychain key for one injected auth header:
/// `browser.header.{profile}.{host}.{name}`.
#[must_use]
pub fn header_secret_name(profile: &str, host: &str, header: &str) -> String {
    format!("browser.header.{profile}.{host}.{header}")
}

/// One cookie to seed into the browser context (value is secret).
#[derive(Debug, Clone)]
pub struct CookieInjection {
    /// Cookie name (safe to log).
    pub name: String,
    /// Cookie value (SECRET — never logged, never ledgered).
    pub value: String,
    /// Cookie domain scope.
    pub domain: String,
    /// Cookie path scope.
    pub path: String,
}

/// One auth header to seed into the browser context (value is secret).
#[derive(Debug, Clone)]
pub struct HeaderInjection {
    /// Header name (safe to log, e.g. `Authorization`).
    pub name: String,
    /// Header value (SECRET — never logged, never ledgered).
    pub value: String,
}

/// Full injection plan for one browser launch.
#[derive(Debug, Clone, Default)]
pub struct InjectionPlan {
    /// Cookies to seed (values secret).
    pub cookies: Vec<CookieInjection>,
    /// Headers to seed (values secret).
    pub headers: Vec<HeaderInjection>,
    /// Profile and host the plan was built for (safe to log).
    pub profile: String,
    /// Host scope the plan was built for (safe to log).
    pub host: String,
}

impl InjectionPlan {
    /// Ledger-safe summary: names + counts only, never values.
    #[must_use]
    pub fn redacted_summary(&self) -> serde_json::Value {
        serde_json::json!({
            "profile": self.profile,
            "host": self.host,
            "cookies": self.cookies.iter().map(|cookie| {
                serde_json::json!({ "name": cookie.name, "domain": cookie.domain, "path": cookie.path })
            }).collect::<Vec<_>>(),
            "headers": self.headers.iter().map(|header| header.name.clone()).collect::<Vec<_>>(),
            "cookieCount": self.cookies.len(),
            "headerCount": self.headers.len(),
        })
    }
}

/// Handoff file the Playwright worker reads at context launch
/// (`<profile_dir>/injections.json`, mode `0o600` on unix).
const INJECTIONS_FILE: &str = "injections.json";

/// Persist an injection plan for the worker handoff.
///
/// Values are written to a `0o600` file inside the Gateway profile dir so
/// they never cross argv, logs, receipts, or events. Missing parent dirs are
/// created; an existing handoff file is replaced. Returns the file path
/// (safe to log) — never the values.
pub async fn write_injection_file(
    profile_dir: &std::path::Path,
    plan: &InjectionPlan,
) -> Result<std::path::PathBuf> {
    tokio::fs::create_dir_all(profile_dir)
        .await
        .map_err(|error| err(format!("injection handoff: {error}")))?;
    let cookies: Vec<serde_json::Value> = plan
        .cookies
        .iter()
        .map(|cookie| {
            serde_json::json!({
                "name": cookie.name,
                "value": cookie.value,
                "domain": cookie.domain,
                "path": cookie.path,
            })
        })
        .collect();
    let mut headers = serde_json::Map::new();
    for header in &plan.headers {
        headers.insert(
            header.name.clone(),
            serde_json::Value::String(header.value.clone()),
        );
    }
    let path = profile_dir.join(INJECTIONS_FILE);
    tokio::fs::write(
        &path,
        serde_json::to_string(&serde_json::json!({ "cookies": cookies, "headers": headers }))
            .unwrap_or_default(),
    )
    .await
    .map_err(|error| err(format!("injection handoff: {error}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        tokio::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .await
            .map_err(|error| err(format!("injection handoff: {error}")))?;
    }
    Ok(path)
}

fn validate_injection_name(what: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > 128 {
        return Err(err(format!("{what} must be 1..=128 chars")));
    }
    Ok(())
}

/// Resolve cookie values from the keychain service into an injection plan.
///
/// `cookie_names` are bare cookie names; the keychain key is derived with
/// [`cookie_secret_name`]. Missing entries are skipped (the login flow still
/// runs); keychain outages error. Values never appear in the error.
pub fn resolve_cookie_injections(
    vault: &talos_secrets::SecretVault,
    service: &str,
    profile: &str,
    host: &str,
    cookie_names: &[String],
    domain: &str,
) -> Result<Vec<CookieInjection>> {
    crate::profile::sanitize_profile_name(profile)?;
    if cookie_names.len() > 32 {
        return Err(err("at most 32 cookies per injection plan"));
    }
    let mut out = Vec::new();
    for name in cookie_names {
        validate_injection_name("cookie name", name)?;
        let key = cookie_secret_name(profile, host, name);
        let value = vault
            .get_secret(service, &key)?
            .ok_or_else(|| err(format!("cookie secret not found: {key}")))?;
        out.push(CookieInjection {
            name: name.clone(),
            value,
            domain: domain.to_owned(),
            path: "/".to_owned(),
        });
    }
    Ok(out)
}

/// Resolve auth-header values from the keychain service.
///
/// Same secrecy contract as [`resolve_cookie_injections`].
pub fn resolve_header_injections(
    vault: &talos_secrets::SecretVault,
    service: &str,
    profile: &str,
    host: &str,
    header_names: &[String],
) -> Result<Vec<HeaderInjection>> {
    crate::profile::sanitize_profile_name(profile)?;
    if header_names.len() > 16 {
        return Err(err("at most 16 headers per injection plan"));
    }
    let mut out = Vec::new();
    for name in header_names {
        validate_injection_name("header name", name)?;
        let key = header_secret_name(profile, host, name);
        let value = vault
            .get_secret(service, &key)?
            .ok_or_else(|| err(format!("header secret not found: {key}")))?;
        out.push(HeaderInjection {
            name: name.clone(),
            value,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault_with_cookie() -> talos_secrets::SecretVault {
        let vault = talos_secrets::SecretVault::mock();
        vault
            .store_secret(
                talos_secrets::TALOS_SERVICE,
                &cookie_secret_name("default", "example.com", "session"),
                "s3cret-cookie-value",
            )
            .expect("store");
        vault
    }

    #[test]
    fn cookie_plan_resolves_without_leaking_values() {
        let vault = vault_with_cookie();
        let cookies = resolve_cookie_injections(
            &vault,
            talos_secrets::TALOS_SERVICE,
            "default",
            "example.com",
            &["session".to_owned()],
            "example.com",
        )
        .expect("resolve");
        assert_eq!(cookies.len(), 1);
        assert_eq!(cookies[0].value, "s3cret-cookie-value");
        // The only ledger-safe rendering carries names, never values.
        let summary = InjectionPlan {
            cookies,
            headers: Vec::new(),
            profile: "default".to_owned(),
            host: "example.com".to_owned(),
        }
        .redacted_summary();
        let rendered = serde_json::to_string(&summary).expect("json");
        assert!(rendered.contains("session"));
        assert!(
            !rendered.contains("s3cret-cookie-value"),
            "values must never reach the ledger: {rendered}"
        );
    }

    #[test]
    fn missing_cookie_names_the_key_not_the_value() {
        let vault = talos_secrets::SecretVault::mock();
        let error = resolve_cookie_injections(
            &vault,
            talos_secrets::TALOS_SERVICE,
            "default",
            "example.com",
            &["session".to_owned()],
            "example.com",
        )
        .expect_err("missing");
        let message = error.to_string();
        assert!(message.contains("browser.cookie.default.example.com.session"));
    }

    #[tokio::test]
    async fn injection_file_round_trips_without_leaking_elsewhere() {
        let vault = vault_with_cookie();
        let dir = std::env::temp_dir().join(format!(
            "talos-injection-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0)
        ));
        let plan = InjectionPlan {
            cookies: resolve_cookie_injections(
                &vault,
                talos_secrets::TALOS_SERVICE,
                "default",
                "example.com",
                &["session".to_owned()],
                "example.com",
            )
            .expect("resolve"),
            headers: Vec::new(),
            profile: "default".to_owned(),
            host: "example.com".to_owned(),
        };
        let path = write_injection_file(&dir, &plan).await.expect("write");
        assert_eq!(
            path.file_name().and_then(|n| n.to_str()),
            Some("injections.json")
        );
        // The worker-readable shape carries values; the ledger-safe shape does not.
        let raw = tokio::fs::read_to_string(&path).await.expect("read");
        let parsed: serde_json::Value = serde_json::from_str(&raw).expect("json");
        assert_eq!(parsed["cookies"][0]["name"], "session");
        let summary = serde_json::to_string(&plan.redacted_summary()).expect("json");
        assert!(!summary.contains("s3cret-cookie-value"));
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[test]
    fn header_plan_naming_and_caps() {
        assert_eq!(
            header_secret_name("default", "example.com", "Authorization"),
            "browser.header.default.example.com.Authorization"
        );
        let vault = talos_secrets::SecretVault::mock();
        let too_many: Vec<String> = (0..17).map(|i| format!("h{i}")).collect();
        assert!(resolve_header_injections(
            &vault,
            talos_secrets::TALOS_SERVICE,
            "default",
            "example.com",
            &too_many
        )
        .is_err());
    }
}
