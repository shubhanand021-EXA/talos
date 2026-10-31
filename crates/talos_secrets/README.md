# `talos_secrets` — OS-keychain secret storage

BYOK key vault for Talos provider keys. Values live in the OS credential
store (Windows Credential Manager, macOS Keychain, Linux Secret Service)
under service `com.talos.desktop` with key names
`provider.{provider_id}.api_key`.

## API

```rust
SecretVault::os().store_secret(service, key_name, value)?;
SecretVault::os().get_secret(service, key_name)?; // Ok(None) when absent
SecretVault::os().delete_secret(service, key_name)?; // missing = success
SecretVault::os().list_secret_names(service)?; // names only, never values
```

Free functions (`store_secret`, `get_secret`, `delete_secret`,
`list_secret_names`) delegate to the OS vault. Tests use
`SecretVault::mock()` (in-memory, hermetic).

## Guarantees

- Values are never logged (tracing records key names only), never placed in
  evidence payloads, IPC responses, events, or error strings.
- Missing entries yield `SecretUnavailable` at the gateway boundary, never
  a value and never a backend path.
- Key names are restricted to `a-z A-Z 0-9 . _ -` (1..=128 chars).
