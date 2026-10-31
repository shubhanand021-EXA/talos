//! OS-keychain secret storage for Talos BYOK provider keys.
//!
//! Secrets live in the OS credential store (Windows Credential Manager,
//! macOS Keychain, Linux Secret Service) behind the [`SecretVault`] API.
//! Secret *values* are never logged, never placed in evidence payloads, IPC
//! responses, events, or error messages — only key *names* travel outside
//! the vault.
//!
//! [`SecretVault::mock`] provides an in-memory backend for hermetic tests.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use talos_common::{Result, TalosError};

/// Service namespace isolating Talos entries in the OS keychain.
pub const TALOS_SERVICE: &str = "com.talos.desktop";

/// Keychain index entry holding the JSON list of stored key names.
const KEY_INDEX_NAME: &str = "_talos_key_index";

/// Backend errors. Values are never included.
#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    /// The OS credential store is unreachable on this machine.
    #[error("secret store unavailable: {0}")]
    Unavailable(String),
    /// No entry exists under this service/key name.
    #[error("secret not found")]
    NotFound,
    /// The stored value failed an internal integrity check.
    #[error("secret backend failure: {0}")]
    Backend(String),
}

impl From<SecretError> for TalosError {
    fn from(err: SecretError) -> Self {
        match err {
            SecretError::NotFound => TalosError::Storage("secret not found".to_owned()),
            SecretError::Unavailable(reason) | SecretError::Backend(reason) => {
                TalosError::Storage(format!("secret store unavailable: {reason}"))
            }
        }
    }
}

/// Pluggable credential backend. The OS backend wraps the `keyring` crate;
/// tests use the in-memory mock.
pub trait SecretBackend: std::fmt::Debug + Send + Sync {
    /// Persist `value` under (`service`, `name`).
    fn store(&self, service: &str, name: &str, value: &str)
        -> std::result::Result<(), SecretError>;
    /// Fetch the value, or `Ok(None)` when no entry exists.
    fn retrieve(
        &self,
        service: &str,
        name: &str,
    ) -> std::result::Result<Option<String>, SecretError>;
    /// Remove the entry. Missing entries are a no-op success.
    fn delete(&self, service: &str, name: &str) -> std::result::Result<(), SecretError>;
    /// List stored key names for `service` (metadata only, never values).
    fn list(&self, service: &str) -> std::result::Result<Vec<String>, SecretError>;
}

/// OS-keychain backend (Windows Credential Manager, macOS Keychain,
/// Linux Secret Service). An index entry tracks key names because the
/// underlying stores expose no enumeration API.
#[derive(Debug, Default)]
pub struct OsBackend;

impl OsBackend {
    fn index_entry(service: &str) -> std::result::Result<keyring::Entry, SecretError> {
        keyring::Entry::new(service, KEY_INDEX_NAME)
            .map_err(|err| SecretError::Unavailable(err.to_string()))
    }

    fn read_index(service: &str) -> std::result::Result<HashSet<String>, SecretError> {
        let entry = Self::index_entry(service)?;
        match entry.get_password() {
            Ok(raw) => serde_json::from_str::<Vec<String>>(&raw)
                .map(HashSet::from_iter)
                .map_err(|err| SecretError::Backend(format!("key index corrupt: {err}"))),
            Err(keyring::Error::NoEntry) => Ok(HashSet::new()),
            Err(err) => Err(SecretError::Unavailable(err.to_string())),
        }
    }

    fn write_index(service: &str, names: &HashSet<String>) -> std::result::Result<(), SecretError> {
        let mut sorted: Vec<&String> = names.iter().collect();
        sorted.sort();
        let raw = serde_json::to_string(&sorted)
            .map_err(|err| SecretError::Backend(format!("key index encode: {err}")))?;
        Self::index_entry(service)?
            .set_password(&raw)
            .map_err(|err| SecretError::Unavailable(err.to_string()))
    }
}

impl SecretBackend for OsBackend {
    fn store(
        &self,
        service: &str,
        name: &str,
        value: &str,
    ) -> std::result::Result<(), SecretError> {
        let mut index = Self::read_index(service)?;
        keyring::Entry::new(service, name)
            .map_err(|err| SecretError::Unavailable(err.to_string()))?
            .set_password(value)
            .map_err(|err| SecretError::Unavailable(err.to_string()))?;
        index.insert(name.to_owned());
        Self::write_index(service, &index)?;
        tracing::debug!(service, key_name = name, "secret stored");
        Ok(())
    }

    fn retrieve(
        &self,
        service: &str,
        name: &str,
    ) -> std::result::Result<Option<String>, SecretError> {
        match keyring::Entry::new(service, name)
            .map_err(|err| SecretError::Unavailable(err.to_string()))?
            .get_password()
        {
            Ok(value) => {
                tracing::debug!(service, key_name = name, "secret retrieved");
                Ok(Some(value))
            }
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(SecretError::Unavailable(err.to_string())),
        }
    }

    fn delete(&self, service: &str, name: &str) -> std::result::Result<(), SecretError> {
        match keyring::Entry::new(service, name)
            .map_err(|err| SecretError::Unavailable(err.to_string()))?
            .delete_credential()
        {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(err) => return Err(SecretError::Unavailable(err.to_string())),
        }
        let mut index = Self::read_index(service)?;
        index.remove(name);
        Self::write_index(service, &index)?;
        tracing::debug!(service, key_name = name, "secret deleted");
        Ok(())
    }

    fn list(&self, service: &str) -> std::result::Result<Vec<String>, SecretError> {
        let mut names: Vec<String> = Self::read_index(service)?.into_iter().collect();
        names.sort();
        Ok(names)
    }
}

/// Hermetic in-memory backend for unit tests. Never touches the OS store.
#[derive(Debug, Default)]
pub struct MockBackend {
    entries: Mutex<HashMap<(String, String), String>>,
}

impl MockBackend {
    /// Create an empty mock backend.
    pub fn new() -> Self {
        Self::default()
    }
}

impl SecretBackend for MockBackend {
    fn store(
        &self,
        service: &str,
        name: &str,
        value: &str,
    ) -> std::result::Result<(), SecretError> {
        self.entries
            .lock()
            .map_err(|err| SecretError::Backend(err.to_string()))?
            .insert((service.to_owned(), name.to_owned()), value.to_owned());
        Ok(())
    }

    fn retrieve(
        &self,
        service: &str,
        name: &str,
    ) -> std::result::Result<Option<String>, SecretError> {
        Ok(self
            .entries
            .lock()
            .map_err(|err| SecretError::Backend(err.to_string()))?
            .get(&(service.to_owned(), name.to_owned()))
            .cloned())
    }

    fn delete(&self, service: &str, name: &str) -> std::result::Result<(), SecretError> {
        self.entries
            .lock()
            .map_err(|err| SecretError::Backend(err.to_string()))?
            .remove(&(service.to_owned(), name.to_owned()));
        Ok(())
    }

    fn list(&self, service: &str) -> std::result::Result<Vec<String>, SecretError> {
        let mut names: Vec<String> = self
            .entries
            .lock()
            .map_err(|err| SecretError::Backend(err.to_string()))?
            .keys()
            .filter(|(svc, _)| svc == service)
            .map(|(_, name)| name.clone())
            .collect();
        names.sort();
        Ok(names)
    }
}

/// Keychain vault. Values enter and leave only through this API; every other
/// layer (IPC, events, evidence, logs) sees key names at most.
#[derive(Debug, Clone)]
pub struct SecretVault {
    backend: Arc<dyn SecretBackend>,
}

impl SecretVault {
    /// Vault backed by the OS keychain.
    pub fn os() -> Self {
        Self {
            backend: Arc::new(OsBackend),
        }
    }

    /// Vault backed by an in-memory store (tests only).
    pub fn mock() -> Self {
        Self {
            backend: Arc::new(MockBackend::new()),
        }
    }

    /// Persist a secret. Overwrites any existing entry under the same name.
    pub fn store_secret(&self, service: &str, key_name: &str, value: &str) -> Result<()> {
        validate_key_name(key_name)?;
        if value.is_empty() {
            return Err(TalosError::InvalidId {
                what: "secret value",
                reason: "value must not be empty".to_owned(),
            });
        }
        self.backend
            .store(service, key_name, value)
            .map_err(TalosError::from)
    }

    /// Fetch a secret value, or `Ok(None)` when absent.
    pub fn get_secret(&self, service: &str, key_name: &str) -> Result<Option<String>> {
        validate_key_name(key_name)?;
        self.backend
            .retrieve(service, key_name)
            .map_err(TalosError::from)
    }

    /// Remove a secret. Missing entries succeed silently.
    pub fn delete_secret(&self, service: &str, key_name: &str) -> Result<()> {
        validate_key_name(key_name)?;
        self.backend
            .delete(service, key_name)
            .map_err(TalosError::from)
    }

    /// List stored key names for a service (metadata only).
    pub fn list_secret_names(&self, service: &str) -> Result<Vec<String>> {
        validate_service(service)?;
        self.backend.list(service).map_err(TalosError::from)
    }
}

impl Default for SecretVault {
    fn default() -> Self {
        Self::os()
    }
}

/// Canonical key name for a provider API key: `provider.{provider_id}.api_key`.
pub fn provider_key_name(provider_id: &str) -> String {
    format!("provider.{provider_id}.api_key")
}

/// Store a provider API key in the Talos OS-keychain service.
pub fn store_secret(service: &str, key_name: &str, value: &str) -> Result<()> {
    SecretVault::os().store_secret(service, key_name, value)
}

/// Fetch a provider secret from the Talos OS-keychain service.
pub fn get_secret(service: &str, key_name: &str) -> Result<Option<String>> {
    SecretVault::os().get_secret(service, key_name)
}

/// Remove a secret from the Talos OS-keychain service.
pub fn delete_secret(service: &str, key_name: &str) -> Result<()> {
    SecretVault::os().delete_secret(service, key_name)
}

/// List secret key names in the Talos OS-keychain service.
pub fn list_secret_names(service: &str) -> Result<Vec<String>> {
    SecretVault::os().list_secret_names(service)
}

fn validate_key_name(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > 128 {
        return Err(TalosError::InvalidId {
            what: "secret key_name",
            reason: "must be 1..=128 chars".to_owned(),
        });
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
    {
        return Err(TalosError::InvalidId {
            what: "secret key_name",
            reason: "allowed chars: a-z A-Z 0-9 . _ -".to_owned(),
        });
    }
    Ok(())
}

fn validate_service(service: &str) -> Result<()> {
    if service.is_empty() || service.len() > 128 {
        return Err(TalosError::InvalidId {
            what: "secret service",
            reason: "must be 1..=128 chars".to_owned(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault() -> SecretVault {
        SecretVault::mock()
    }

    #[test]
    fn store_retrieve_delete_cycle() {
        let vault = vault();
        assert_eq!(vault.get_secret(TALOS_SERVICE, "cycle.key").unwrap(), None);
        vault
            .store_secret(TALOS_SERVICE, "cycle.key", "s3cret-value")
            .unwrap();
        assert_eq!(
            vault.get_secret(TALOS_SERVICE, "cycle.key").unwrap(),
            Some("s3cret-value".to_owned())
        );
        vault.delete_secret(TALOS_SERVICE, "cycle.key").unwrap();
        assert_eq!(vault.get_secret(TALOS_SERVICE, "cycle.key").unwrap(), None);
        // Deleting a missing entry succeeds silently.
        vault.delete_secret(TALOS_SERVICE, "cycle.key").unwrap();
    }

    #[test]
    fn overwrite_replaces_value() {
        let vault = vault();
        vault.store_secret(TALOS_SERVICE, "k", "first").unwrap();
        vault.store_secret(TALOS_SERVICE, "k", "second").unwrap();
        assert_eq!(
            vault.get_secret(TALOS_SERVICE, "k").unwrap(),
            Some("second".to_owned())
        );
    }

    #[test]
    fn list_returns_names_only_sorted() {
        let vault = vault();
        vault.store_secret(TALOS_SERVICE, "b.key", "x").unwrap();
        vault.store_secret(TALOS_SERVICE, "a.key", "y").unwrap();
        vault.store_secret("other.service", "c.key", "z").unwrap();
        assert_eq!(
            vault.list_secret_names(TALOS_SERVICE).unwrap(),
            vec!["a.key".to_owned(), "b.key".to_owned()]
        );
        vault.delete_secret(TALOS_SERVICE, "a.key").unwrap();
        assert_eq!(
            vault.list_secret_names(TALOS_SERVICE).unwrap(),
            vec!["b.key".to_owned()]
        );
    }

    #[test]
    fn rejects_empty_values_and_bad_names() {
        let vault = vault();
        assert!(vault.store_secret(TALOS_SERVICE, "k", "").is_err());
        assert!(vault.store_secret(TALOS_SERVICE, "", "x").is_err());
        assert!(vault.store_secret(TALOS_SERVICE, "has space", "x").is_err());
        assert!(vault.get_secret(TALOS_SERVICE, "").is_err());
        assert!(vault.list_secret_names("").is_err());
    }

    #[test]
    fn provider_key_naming_convention() {
        assert_eq!(provider_key_name("openai"), "provider.openai.api_key");
        assert_eq!(provider_key_name("anthropic"), "provider.anthropic.api_key");
        assert_eq!(provider_key_name("ollama"), "provider.ollama.api_key");
    }

    #[test]
    fn error_messages_never_carry_values() {
        let err = SecretVault::mock()
            .store_secret(TALOS_SERVICE, "k", "")
            .unwrap_err()
            .to_string();
        assert!(!err.contains("s3cret"));
        assert_eq!(
            TalosError::from(SecretError::NotFound).to_string(),
            "storage error: secret not found"
        );
    }
}
