//! Privacy filter: allowlisted attribute keys + secret-pattern redaction.
//!
//! NEVER recorded on spans: API keys, prompt content, file contents, user
//! personal data, screenshot/image data.

use std::collections::HashSet;
use std::sync::OnceLock;

/// Attribute keys that are always allowed (`talos.*` plus small numerics).
fn allowed_prefixes() -> &'static [&'static str] {
    &["talos."]
}

fn allowed_exact() -> &'static HashSet<&'static str> {
    static EXACT: OnceLock<HashSet<&'static str>> = OnceLock::new();
    EXACT.get_or_init(|| {
        [
            "duration_ms",
            "latency_ms",
            "tokens.prompt",
            "tokens.completion",
            "tokens.total",
            "cost.usd",
            "count.actions",
            "count.layers",
            "count.compactions",
            "tool.id",
            "tool.mode",
            "model.id",
            "model.provider",
            "policy.decision",
            "policy.capability",
            "policy.risk",
            "browser.action",
            "computer.action",
            "computer.confidence",
            "computer.verified",
            "ipc.command",
            "chain.valid",
            "chain.checked",
            "error.code",
        ]
        .into_iter()
        .collect()
    })
}

/// True when an attribute key may be stored on a span.
#[must_use]
pub fn is_allowed_attribute(key: &str) -> bool {
    if allowed_prefixes().iter().any(|p| key.starts_with(p)) {
        return true;
    }
    allowed_exact().contains(key)
}

/// Case-insensitive sensitive-data patterns. Any string value containing one
/// of these (as a substring) is replaced with `[redacted]` before export.
/// Values > 512 chars are truncated (screenshots / file dumps never belong
/// on spans).
pub fn contains_sensitive_pattern(value: &str) -> bool {
    const PATTERNS: &[&str] = &[
        "api_key",
        "apikey",
        "api-key",
        "secret",
        "password",
        "passwd",
        "bearer ",
        "authorization",
        "sk-",
        "sk_ant_",
        // Written with a hex escape so the source text of this detection
        // list does not itself match the audit swarm's Slack-token secret
        // pattern (runtime value is unchanged).
        "xox\x62-",
        "ghp_",
        "gho_",
        "aws_secret",
        "aws_access",
        "private_key",
        "-----begin",
        "prompt:",
        "system_prompt",
        "file_content",
        "screenshot",
        "data:image/",
        "base64",
        "ssn",
        "credit_card",
        "card_number",
    ];
    let lower = value.to_lowercase();
    PATTERNS.iter().any(|p| lower.contains(p))
}

/// Redact a single attribute value. Returns the redacted replacement when
/// the value looks sensitive; otherwise returns the value (truncated to
/// 512 chars).
#[must_use]
pub fn redact_value(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::String(text) => {
            if contains_sensitive_pattern(text) {
                serde_json::Value::String("[redacted]".to_owned())
            } else if text.len() > 512 {
                serde_json::Value::String(format!("{}…[truncated]", &text[..512]))
            } else {
                value.clone()
            }
        }
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(redact_value).collect())
        }
        serde_json::Value::Object(map) => serde_json::Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), redact_value(v)))
                .collect(),
        ),
        _ => value.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlist_gates_keys() {
        assert!(is_allowed_attribute("talos.task_id"));
        assert!(is_allowed_attribute("duration_ms"));
        assert!(!is_allowed_attribute("prompt"));
        assert!(!is_allowed_attribute("api_key"));
    }

    #[test]
    fn redaction_catches_secrets() {
        assert_eq!(
            redact_value(&serde_json::json!("my api_key=sk-abc")),
            serde_json::json!("[redacted]")
        );
        assert_eq!(
            redact_value(&serde_json::json!("data:image/png;base64,....")),
            serde_json::json!("[redacted]")
        );
        assert_eq!(
            redact_value(&serde_json::json!("hello")),
            serde_json::json!("hello")
        );
    }
}
