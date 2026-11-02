//! At-rest encryption for the evidence database (file envelope encryption).
//!
//! Rationale: `sqlx` 0.8 has no SQLCipher feature, so page-level encryption
//! is unavailable. Instead Talos encrypts the whole database file with
//! ChaCha20-Poly1305 (pure Rust, audited `chacha20poly1305` crate) under a
//! 256-bit key held in the OS keychain. SQLite operates on a decrypted
//! working copy; the canonical `talos.db` on disk is always ciphertext when
//! encryption is enabled.
//!
//! Envelope layout: `TALOSENV` (8) + version `0x01` (1) + nonce (12) +
//! ciphertext + tag (16), AEAD with the magic+version as associated data.
//!
//! Honest limitation: while the process runs, the working copy is plaintext
//! (mode `0600` on Unix, per-user app-data dir on Windows, wiped with zeros
//! on clean shutdown). A crash leaves the working copy behind; the next
//! startup detects the orphan, runs crash recovery against it, then
//! re-encrypts. An attacker who can read process memory or the live working
//! file can already read everything — the envelope protects data at rest
//! (stolen disk, powered-off machine, backups).

use std::path::{Path, PathBuf};

use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    ChaCha20Poly1305, Nonce,
};
use talos_common::{Result, TalosError};
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Keychain service holding the database key (matches `TALOS_SERVICE`).
pub const KEY_SERVICE: &str = talos_secrets::TALOS_SERVICE;
/// Keychain entry name for the 256-bit database key (hex).
pub const KEY_NAME: &str = "database.encryption_key";
/// Envelope magic (first 8 bytes of every encrypted file).
pub const ENVELOPE_MAGIC: &[u8; 8] = b"TALOSENV";
/// Envelope format version.
pub const ENVELOPE_VERSION: u8 = 1;
/// SQLite file header (`SQLite format 3\0`).
pub const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";
/// Minimum envelope size: magic + version + nonce + tag.
const ENVELOPE_OVERHEAD: usize = 8 + 1 + 12 + 16;

/// 256-bit database key. Zeroed on drop; never logged or serialized.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct DatabaseKey([u8; 32]);

impl std::fmt::Debug for DatabaseKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DatabaseKey(redacted)")
    }
}

impl DatabaseKey {
    /// Generate a fresh random 256-bit key.
    #[must_use]
    pub fn generate() -> Self {
        // No literal buffer: the array comes straight from the CSPRNG so no
        // hard-coded key material ever exists (also satisfies CodeQL's
        // hard-coded-cryptographic-value rule, which flags zeroed buffers).
        use rand::Rng as _;
        Self(rand::rng().random())
    }

    /// Parse a 64-char hex key (keychain format).
    pub fn from_hex(hex: &str) -> Result<Self> {
        let raw = hex::decode(hex.trim())
            .map_err(|_| TalosError::Storage("database key is not valid hex".to_owned()))?;
        if raw.len() != 32 {
            return Err(TalosError::Storage(format!(
                "database key must be 32 bytes, got {}",
                raw.len()
            )));
        }
        let mut bytes = [0_u8; 32];
        bytes.copy_from_slice(&raw);
        Ok(Self(bytes))
    }

    /// Hex encoding for keychain storage (keychain transport only).
    #[must_use]
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Raw key bytes (AEAD input only; never persisted or logged).
    #[must_use]
    pub fn bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Outcome of resolving the database key from the OS keychain.
#[derive(Debug)]
pub enum KeyState {
    /// Keychain held (or freshly stored) a valid key.
    Encrypted(DatabaseKey),
    /// Keychain unavailable: caller must fall back to plaintext with a loud
    /// warning. Never silent — the reason names the failure.
    Unavailable(String),
}

/// Load the database key from the OS keychain, generating + storing one on
/// first run. Keychain transport failures yield [`KeyState::Unavailable`]
/// (loud fallback path); corrupt stored values are hard errors (fail closed
/// rather than encrypting under a different key than the data uses).
pub fn load_or_create_key(vault: &talos_secrets::SecretVault) -> Result<KeyState> {
    match vault.get_secret(KEY_SERVICE, KEY_NAME) {
        Ok(Some(hex)) => DatabaseKey::from_hex(&hex).map(KeyState::Encrypted),
        Ok(None) => {
            let key = DatabaseKey::generate();
            match vault.store_secret(KEY_SERVICE, KEY_NAME, &key.to_hex()) {
                Ok(()) => Ok(KeyState::Encrypted(key)),
                Err(err) => Ok(KeyState::Unavailable(format!(
                    "keychain store failed: {err}"
                ))),
            }
        }
        Err(err) => Ok(KeyState::Unavailable(format!(
            "keychain unavailable: {err}"
        ))),
    }
}

/// On-disk classification for migration decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    /// Path does not exist yet.
    Missing,
    /// Plaintext SQLite database (migration candidate).
    SqlitePlaintext,
    /// Talos ChaCha20-Poly1305 envelope.
    Envelope,
    /// Neither (refuse to touch).
    Unknown,
}

/// Classify a file by its header bytes (no parsing, no execution).
pub fn classify_file(path: &Path) -> Result<FileKind> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(FileKind::Missing),
        Err(err) => {
            return Err(TalosError::Storage(format!(
                "read {}: {err}",
                path.display()
            )));
        }
    };
    Ok(classify_bytes(&bytes))
}

/// Classify in-memory header bytes (pure; unit-tested).
#[must_use]
pub fn classify_bytes(bytes: &[u8]) -> FileKind {
    if bytes.len() >= 16 && &bytes[..16] == SQLITE_MAGIC {
        FileKind::SqlitePlaintext
    } else if bytes.len() >= 9 && &bytes[..8] == ENVELOPE_MAGIC && bytes[8] == ENVELOPE_VERSION {
        FileKind::Envelope
    } else if bytes.is_empty() {
        FileKind::Missing
    } else {
        FileKind::Unknown
    }
}

fn cipher(key: &DatabaseKey) -> ChaCha20Poly1305 {
    ChaCha20Poly1305::new_from_slice(key.bytes()).expect("32-byte key is valid")
}

fn aad() -> Vec<u8> {
    let mut aad = Vec::with_capacity(9);
    aad.extend_from_slice(ENVELOPE_MAGIC);
    aad.push(ENVELOPE_VERSION);
    aad
}

/// Encrypt bytes into a versioned envelope (fresh random nonce per call).
#[must_use]
pub fn encrypt_bytes(key: &DatabaseKey, plaintext: &[u8]) -> Vec<u8> {
    // Fresh random nonce per call straight from the CSPRNG (never reused,
    // never hard-coded).
    use rand::Rng as _;
    let nonce_bytes: [u8; 12] = rand::rng().random();
    let nonce = Nonce::try_from(nonce_bytes.as_slice()).expect("12-byte nonce fits");
    let ciphertext = cipher(key)
        .encrypt(
            &nonce,
            Payload {
                msg: plaintext,
                aad: &aad(),
            },
        )
        .expect("in-memory encryption cannot fail");
    let mut out = Vec::with_capacity(ENVELOPE_OVERHEAD + plaintext.len());
    out.extend_from_slice(ENVELOPE_MAGIC);
    out.push(ENVELOPE_VERSION);
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    out
}

/// Decrypt an envelope (wrong key, truncation, or tampering → error).
pub fn decrypt_bytes(key: &DatabaseKey, envelope: &[u8]) -> Result<Vec<u8>> {
    if classify_bytes(envelope) != FileKind::Envelope || envelope.len() < 9 + 12 + 16 {
        return Err(TalosError::Storage(
            "not a Talos encrypted envelope".to_owned(),
        ));
    }
    let nonce = Nonce::try_from(&envelope[9..21])
        .map_err(|_| TalosError::Storage("malformed envelope header".to_owned()))?;
    cipher(key)
        .decrypt(
            &nonce,
            Payload {
                msg: &envelope[21..],
                aad: &aad(),
            },
        )
        .map_err(|_| {
            TalosError::Storage("envelope authentication failed (wrong key or tampered)".to_owned())
        })
}

/// Encrypt a file in place (read → encrypt → atomic replace).
pub fn encrypt_file_in_place(path: &Path, key: &DatabaseKey) -> Result<()> {
    let plaintext = std::fs::read(path)
        .map_err(|err| TalosError::Storage(format!("read {}: {err}", path.display())))?;
    let envelope = encrypt_bytes(key, &plaintext);
    atomic_write(path, &envelope)?;
    Ok(())
}

/// Decrypt `src` (envelope) into `dest`, creating parent dirs.
pub fn decrypt_file_to(src: &Path, dest: &Path, key: &DatabaseKey) -> Result<()> {
    let envelope = std::fs::read(src)
        .map_err(|err| TalosError::Storage(format!("read {}: {err}", src.display())))?;
    let plaintext = decrypt_bytes(key, &envelope)?;
    atomic_write(dest, &plaintext)?;
    restrict_file(dest)?;
    Ok(())
}

/// Retry a filesystem mutation that can race lingering handles (Windows
/// file locking, antivirus scanners). Bounded: 100 attempts, 100ms apart.
///
/// Retries `PermissionDenied` plus raw Windows sharing/lock violations
/// (os errors 32/33 surface as `Uncategorized`, so kind-matching alone
/// would never retry them).
pub(crate) fn retry_locked(mut op: impl FnMut() -> std::io::Result<()>) -> std::io::Result<()> {
    fn transient(err: &std::io::Error) -> bool {
        if err.kind() == std::io::ErrorKind::PermissionDenied {
            return true;
        }
        // ERROR_SHARING_VIOLATION / ERROR_LOCK_VIOLATION on Windows.
        matches!(err.raw_os_error(), Some(32) | Some(33))
    }
    let mut attempts = 0;
    loop {
        match op() {
            Ok(()) => return Ok(()),
            Err(err) if transient(&err) && attempts < 100 => {
                attempts += 1;
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(err) => return Err(err),
        }
    }
}

/// Atomic write: temp file in the same directory + rename.
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
    if let Some(parent) = parent {
        std::fs::create_dir_all(parent)
            .map_err(|err| TalosError::Storage(format!("create dir: {err}")))?;
    }
    let tmp = tmp_sibling(path);
    std::fs::write(&tmp, bytes).map_err(|err| TalosError::Storage(format!("write temp: {err}")))?;
    retry_locked(|| std::fs::rename(&tmp, path))
        .map_err(|err| TalosError::Storage(format!("atomic replace: {err}")))?;
    Ok(())
}

/// Sibling temp path for atomic replaces (`<name>.tmp`).
#[must_use]
pub fn tmp_sibling(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(".tmp");
    path.with_file_name(name)
}

/// Best-effort secure wipe: overwrite with zeros, then remove. Returns
/// `Ok` even when the file is already gone. (SSD wear-leveling means this
/// is mitigation, not a guarantee — documented in SECURITY.md.)
pub fn wipe_file(path: &Path) -> Result<()> {
    let Ok(meta) = std::fs::metadata(path) else {
        return Ok(());
    };
    let len = meta.len().min(256 * 1024 * 1024) as usize;
    if let Ok(file) = std::fs::OpenOptions::new().write(true).open(path) {
        const CHUNK: usize = 65536;
        let zeros = vec![0_u8; CHUNK];
        let mut remaining = len;
        use std::io::Write as _;
        let mut file = file;
        while remaining > 0 {
            let n = remaining.min(CHUNK);
            if file.write_all(&zeros[..n]).is_err() {
                break;
            }
            remaining -= n;
        }
        let _ = file.sync_all();
    }
    match retry_locked(|| std::fs::remove_file(path)) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(TalosError::Storage(format!(
            "wipe {}: {err}",
            path.display()
        ))),
    }
}

/// Restrict a file to owner-only access (Unix `0600`; no-op elsewhere where
/// the per-user app-data dir already confines access).
pub fn restrict_file(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|err| TalosError::Storage(format!("restrict {}: {err}", path.display())))?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_hex_round_trip() {
        let key = DatabaseKey::generate();
        let hex = key.to_hex();
        assert_eq!(hex.len(), 64);
        let parsed = DatabaseKey::from_hex(&hex).expect("parses");
        assert_eq!(parsed.bytes(), key.bytes());
        assert!(DatabaseKey::from_hex("zzzz").is_err());
        assert!(DatabaseKey::from_hex(&hex[..62]).is_err());
        // Debug never leaks key material.
        assert!(!format!("{key:?}").contains(&hex));
    }

    #[test]
    fn envelope_round_trip_and_tamper_detection() {
        let key = DatabaseKey::generate();
        let plaintext = b"SQLite format 3\0-sensitive-data";
        let envelope = encrypt_bytes(&key, plaintext);
        assert_eq!(classify_bytes(&envelope), FileKind::Envelope);
        // No plaintext table names or magic survive.
        assert!(!envelope.windows(6).any(|w| w == b"SQLite"));
        assert!(!envelope.windows(9).any(|w| w == b"sensitive"));
        assert_eq!(decrypt_bytes(&key, &envelope).expect("decrypts"), plaintext);
        // Wrong key fails closed.
        let other = DatabaseKey::generate();
        assert!(decrypt_bytes(&other, &envelope).is_err());
        // Tampering fails closed.
        let mut tampered = envelope.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 0x01;
        assert!(decrypt_bytes(&key, &tampered).is_err());
        // Truncation fails closed.
        assert!(decrypt_bytes(&key, &envelope[..20]).is_err());
        // Distinct nonces per encryption (no nonce reuse).
        let again = encrypt_bytes(&key, plaintext);
        assert_ne!(envelope, again);
    }

    #[test]
    fn classify_headers() {
        assert_eq!(classify_bytes(b""), FileKind::Missing);
        assert_eq!(
            classify_bytes(b"SQLite format 3\0rest"),
            FileKind::SqlitePlaintext
        );
        assert_eq!(classify_bytes(b"definitely not sqlite"), FileKind::Unknown);
        let key = DatabaseKey::generate();
        assert_eq!(
            classify_bytes(&encrypt_bytes(&key, b"x")),
            FileKind::Envelope
        );
    }

    #[test]
    fn file_round_trip_and_wipe() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("secret.db");
        std::fs::write(&path, b"SQLite format 3\0data").expect("writes");
        let key = DatabaseKey::generate();
        encrypt_file_in_place(&path, &key).expect("encrypts");
        let bytes = std::fs::read(&path).expect("reads");
        assert_eq!(classify_bytes(&bytes), FileKind::Envelope);
        assert!(!bytes.windows(6).any(|w| w == b"SQLite"));
        let dest = dir.path().join("plain.db");
        decrypt_file_to(&path, &dest, &key).expect("decrypts");
        assert_eq!(
            std::fs::read(&dest).expect("reads"),
            b"SQLite format 3\0data"
        );
        wipe_file(&dest).expect("wipes");
        assert!(!dest.exists());
        wipe_file(&dest).expect("missing wipe is ok");
    }

    struct FailingVault;

    impl std::fmt::Debug for FailingVault {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("FailingVault")
        }
    }

    impl talos_secrets::SecretBackend for FailingVault {
        fn store(
            &self,
            _service: &str,
            _name: &str,
            _value: &str,
        ) -> std::result::Result<(), talos_secrets::SecretError> {
            Err(talos_secrets::SecretError::Backend(
                "no keychain".to_owned(),
            ))
        }

        fn retrieve(
            &self,
            _service: &str,
            _name: &str,
        ) -> std::result::Result<Option<String>, talos_secrets::SecretError> {
            Err(talos_secrets::SecretError::Backend(
                "no keychain".to_owned(),
            ))
        }

        fn delete(
            &self,
            _service: &str,
            _name: &str,
        ) -> std::result::Result<(), talos_secrets::SecretError> {
            Err(talos_secrets::SecretError::Backend(
                "no keychain".to_owned(),
            ))
        }

        fn list(
            &self,
            _service: &str,
        ) -> std::result::Result<Vec<String>, talos_secrets::SecretError> {
            Err(talos_secrets::SecretError::Backend(
                "no keychain".to_owned(),
            ))
        }
    }

    #[test]
    fn keychain_failure_yields_loud_unavailable() {
        use talos_secrets::SecretVault;
        let vault = SecretVault::mock();
        // First run stores a fresh key.
        let first = load_or_create_key(&vault).expect("loads");
        assert!(matches!(first, KeyState::Encrypted(_)));
        // Second run retrieves the same key.
        let second = load_or_create_key(&vault).expect("loads");
        let (KeyState::Encrypted(a), KeyState::Encrypted(b)) = (first, second) else {
            panic!("both runs must be encrypted");
        };
        assert_eq!(a.bytes(), b.bytes());
        // Corrupt stored value fails closed (never encrypts under a new key).
        vault
            .store_secret(KEY_SERVICE, KEY_NAME, "not-hex")
            .expect("stores");
        assert!(load_or_create_key(&vault).is_err());
    }

    #[test]
    fn unavailable_keychain_falls_back_loudly() {
        use std::sync::Arc;
        use talos_secrets::SecretVault;
        let vault = SecretVault::with_backend(Arc::new(FailingVault));
        match load_or_create_key(&vault).expect("maps failure to Unavailable") {
            KeyState::Unavailable(reason) => {
                assert!(reason.contains("no keychain"), "{reason}");
            }
            KeyState::Encrypted(_) => panic!("failing backend must not yield a key"),
        }
    }
}
