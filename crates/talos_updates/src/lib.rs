//! Talos release-update logic shared by the desktop updater IPC and the
//! headless CLI.
//!
//! The desktop `updater.rs` IPC layer (`check_for_updates`,
//! `download_update`, `install_update`, channel commands) calls into this
//! crate for everything testable without a Tauri `AppHandle`: channel
//! parsing, endpoint expansion, signed-manifest validation, and the
//! deterministic staged-rollout gate. Keeping the logic here means the
//! workspace `cargo test` gate (which CI runs) covers it; the desktop shell
//! only wires bytes to the screen.
//!
//! Verification is two-layered: [`verify_manifest_signature`] checks the
//! minisign `.sig` over a release manifest (same format produced offline by
//! `scripts/sign-release.mjs`), and the Tauri updater plugin verifies every
//! installed bundle against the pubkey pinned in `tauri.conf.json` before it
//! touches the installation.

use talos_common::{Result, TalosError};

/// Release channel. Beta/nightly builds ship with their own endpoint; the
/// preference is persisted in shared `app_state` so the CLI and desktop
/// agree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateChannel {
    /// Production releases (default).
    Stable,
    /// Pre-release validation.
    Beta,
    /// Nightly development snapshots.
    Nightly,
}

impl UpdateChannel {
    /// Canonical channel name used in endpoints and persisted state.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Beta => "beta",
            Self::Nightly => "nightly",
        }
    }

    /// Parse a persisted channel name (unknown values fall back to stable —
    /// a corrupt preference must never brick updates).
    #[must_use]
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_lowercase().as_str() {
            "beta" => Self::Beta,
            "nightly" => Self::Nightly,
            _ => Self::Stable,
        }
    }
}

/// Default release channel.
pub const DEFAULT_CHANNEL: &str = "stable";

/// Persisted install identifier (stable rollout bucket + diagnostics).
pub const INSTALL_ID_KEY: &str = "install_id";

/// Placeholder update endpoint until the first release channel exists.
pub const UPDATE_ENDPOINT_TEMPLATE: &str =
    "https://updates.talos.dev/{{channel}}/{{target}}/{{arch}}/{{current_version}}";

/// Expand an update-endpoint template (`{{channel}}`, `{{target}}`,
/// `{{arch}}`, `{{current_version}}`) for display and diagnostics.
#[must_use]
pub fn expand_endpoint_template(
    template: &str,
    channel: &str,
    target: &str,
    arch: &str,
    current_version: &str,
) -> String {
    template
        .replace("{{channel}}", channel)
        .replace("{{target}}", target)
        .replace("{{arch}}", arch)
        .replace("{{current_version}}", current_version)
}

/// One downloadable artifact inside a release manifest.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ManifestArtifact {
    /// Tauri target triple (`windows-x86_64`, `darwin-aarch64`, ...).
    pub target: String,
    /// Architecture (`x86_64`, `aarch64`).
    pub arch: String,
    /// Bundle download URL.
    pub url: String,
    /// Minisign `.sig` text for the bundle (verified before staging).
    pub signature: String,
}

/// Signed release manifest published per channel + version.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UpdateManifest {
    /// Release version (semver).
    pub version: String,
    /// Release notes (Markdown, rendered verbatim).
    pub notes: Option<String>,
    /// Publication date (RFC 3339).
    pub pub_date: Option<String>,
    /// Staged-rollout percentage (0–100). `None` = fully rolled out.
    pub rollout_percentage: Option<u8>,
    /// Downloadable artifacts.
    pub artifacts: Vec<ManifestArtifact>,
}

/// Parse and validate a release manifest (structural checks only; signature
/// verification is [`verify_manifest_signature`]).
pub fn parse_update_manifest(json: &str) -> Result<UpdateManifest> {
    let manifest: UpdateManifest = serde_json::from_str(json)
        .map_err(|err| TalosError::Storage(format!("malformed manifest: {err}")))?;
    if manifest.version.trim().is_empty() || manifest.version.len() > 64 {
        return Err(TalosError::Storage(
            "manifest version must be 1..=64 chars".to_owned(),
        ));
    }
    if let Some(pct) = manifest.rollout_percentage {
        if pct > 100 {
            return Err(TalosError::Storage(
                "rollout_percentage must be 0..=100".to_owned(),
            ));
        }
    }
    if manifest.artifacts.is_empty() {
        return Err(TalosError::Storage(
            "manifest must list at least one artifact".to_owned(),
        ));
    }
    for artifact in &manifest.artifacts {
        if artifact.target.trim().is_empty() || artifact.url.trim().is_empty() {
            return Err(TalosError::Storage(
                "artifact target and url must not be empty".to_owned(),
            ));
        }
    }
    Ok(manifest)
}

/// Deterministic staged-rollout gate: hashes `install_id` into a 0–99 bucket
/// and admits only buckets below `rollout_percentage` (`None` = everyone).
/// Same install id → same verdict forever (no flapping mid-rollout).
#[must_use]
pub fn rollout_applies(rollout_percentage: Option<u8>, install_id: &str) -> bool {
    match rollout_percentage {
        None => true,
        Some(pct) => {
            use sha2::Digest as _;
            let digest = sha2::Sha256::digest(install_id.as_bytes());
            (digest[0] % 100) < pct
        }
    }
}

/// Verify a minisign `.sig` text over `payload` against a base64 public key
/// (the same `.pub` second-line format `scripts/sign-release.mjs` emits and
/// the updater pubkey pinned in `tauri.conf.json`).
///
/// Both the file signature and the trusted-comment global signature must
/// verify; key-id mismatch, tampered bytes, and wrong keys all fail closed.
pub fn verify_manifest_signature(payload: &[u8], sig_text: &str, pubkey_b64: &str) -> Result<()> {
    let public_key = minisign_verify::PublicKey::from_base64(pubkey_b64)
        .map_err(|err| TalosError::Policy(format!("invalid public key: {err:?}")))?;
    let signature = minisign_verify::Signature::decode(sig_text)
        .map_err(|err| TalosError::Policy(format!("invalid signature: {err:?}")))?;
    // `allow_legacy = true`: release signatures are plain (non-prehashed)
    // ed25519 over the payload, matching `sign-release.mjs` output.
    public_key
        .verify(payload, &signature, true)
        .map_err(|err| TalosError::Policy(format!("signature rejected: {err:?}")))
}

/// Read the channel preference from shared state (defaults to stable).
pub async fn read_channel(db: &evidence_store::EvidenceDb) -> String {
    match evidence_store::get_app_state(db, evidence_store::app_state::keys::UPDATE_CHANNEL).await {
        Ok(Some(raw)) => UpdateChannel::parse(&raw).as_str().to_owned(),
        _ => DEFAULT_CHANNEL.to_owned(),
    }
}

/// Stable install identifier for rollout bucketing (created once, then
/// reused forever).
pub async fn install_id(db: &evidence_store::EvidenceDb) -> Result<String> {
    if let Ok(Some(existing)) = evidence_store::get_app_state(db, INSTALL_ID_KEY).await {
        if !existing.trim().is_empty() {
            return Ok(existing);
        }
    }
    let fresh = uuid::Uuid::new_v4().to_string();
    evidence_store::set_app_state(db, INSTALL_ID_KEY, &fresh).await?;
    Ok(fresh)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Deterministic vectors: `payload.bin` = b"talos-update-manifest-test-v1",
    // signed offline with `scripts/sign-release.mjs` (test key, since
    // destroyed — NEVER a production key).
    const TEST_PAYLOAD: &[u8] = b"talos-update-manifest-test-v1";
    const TEST_PUBKEY_B64: &str = "RWSKEJx0I9E4GbT03dHtUxpa9aWdMiqAQd1bzhSiCMgKSRVHaMfhtfOU";
    const TEST_SIG: &str = "untrusted comment: signature from talos release key\nRWSKEJx0I9E4GQYItcVyag1I99ucQTxLDQQZfGZ5t4aMH9AaKOAN0zrDF0b+IWvyS0FIoUHimhbdVqPGAM1cgILqpq6kVQjfagI=\ntrusted comment: timestamp:1793629347\tfile=payload.bin\nFVo+RohxSPpe9qk/jhHZTnNLK7m8t4AxsRfzmeEV2x4o0BXhTAPfRtvIOKNaxvlWUjJTG3KGOY/YPsnUDwbjDQ==";

    #[test]
    fn endpoint_template_expands_with_channel() {
        assert_eq!(
            expand_endpoint_template(
                UPDATE_ENDPOINT_TEMPLATE,
                "beta",
                "windows-x86_64",
                "x86_64",
                "0.4.0"
            ),
            "https://updates.talos.dev/beta/windows-x86_64/x86_64/0.4.0"
        );
        // Unknown placeholders pass through untouched (`.test` TLD: RFC 2606
        // documentation-only, never a runtime target).
        assert_eq!(
            expand_endpoint_template("https://updater.test/{{nope}}", "s", "t", "a", "v"),
            "https://updater.test/{{nope}}"
        );
    }

    #[test]
    fn channel_parses_with_stable_fallback() {
        assert_eq!(UpdateChannel::parse("beta"), UpdateChannel::Beta);
        assert_eq!(UpdateChannel::parse("NIGHTLY"), UpdateChannel::Nightly);
        assert_eq!(UpdateChannel::parse("stable"), UpdateChannel::Stable);
        assert_eq!(UpdateChannel::parse("bogus"), UpdateChannel::Stable);
        assert_eq!(UpdateChannel::parse(""), UpdateChannel::Stable);
    }

    #[test]
    fn manifest_parses_and_rejects_bad_rollout() {
        let json = serde_json::json!({
            "version": "0.5.0",
            "notes": "hardening",
            "pub_date": "2026-11-02T00:00:00Z",
            "rollout_percentage": 25,
            "artifacts": [
                {"target": "windows-x86_64", "arch": "x86_64",
                 "url": "https://updates.talos.dev/x.msi", "signature": "sig"}
            ],
        })
        .to_string();
        let manifest = parse_update_manifest(&json).expect("parses");
        assert_eq!(manifest.version, "0.5.0");
        assert_eq!(manifest.rollout_percentage, Some(25));
        assert_eq!(manifest.artifacts.len(), 1);

        let bad_pct = serde_json::json!({
            "version": "0.5.0", "rollout_percentage": 101,
            "artifacts": [{"target": "t", "arch": "a", "url": "u", "signature": "s"}],
        })
        .to_string();
        assert!(parse_update_manifest(&bad_pct).is_err());

        let no_artifacts = serde_json::json!({"version": "0.5.0", "artifacts": []}).to_string();
        assert!(parse_update_manifest(&no_artifacts).is_err());
        assert!(parse_update_manifest("not json").is_err());
    }

    #[test]
    fn rollout_gate_is_deterministic() {
        // `None` admits everyone; 0 admits no one; 100 admits everyone.
        assert!(rollout_applies(None, "any-install"));
        assert!(!rollout_applies(Some(0), "install-1"));
        assert!(rollout_applies(Some(100), "install-1"));
        // Same install id → same verdict (no mid-rollout flapping).
        let first = rollout_applies(Some(25), "stable-install-id");
        for _ in 0..10 {
            assert_eq!(rollout_applies(Some(25), "stable-install-id"), first);
        }
    }

    #[test]
    fn manifest_signature_verifies() {
        verify_manifest_signature(TEST_PAYLOAD, TEST_SIG, TEST_PUBKEY_B64).expect("valid sig");
    }

    #[test]
    fn manifest_signature_rejects_tampered_payload() {
        let mut tampered = TEST_PAYLOAD.to_vec();
        tampered[0] ^= 0x01;
        assert!(verify_manifest_signature(&tampered, TEST_SIG, TEST_PUBKEY_B64).is_err());
    }

    #[test]
    fn manifest_signature_rejects_wrong_key() {
        // Different key id (all-zero ed25519 key): must fail closed.
        let other = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";
        assert!(verify_manifest_signature(TEST_PAYLOAD, TEST_SIG, other).is_err());
    }

    #[test]
    fn manifest_signature_rejects_garbage() {
        assert!(verify_manifest_signature(TEST_PAYLOAD, "garbage", TEST_PUBKEY_B64).is_err());
        assert!(verify_manifest_signature(TEST_PAYLOAD, TEST_SIG, "!!!").is_err());
    }

    #[tokio::test]
    async fn channel_and_install_id_round_trip() {
        let db = evidence_store::EvidenceDb::connect_memory()
            .await
            .expect("connects");
        assert_eq!(read_channel(&db).await, "stable");
        evidence_store::set_app_state(&db, evidence_store::app_state::keys::UPDATE_CHANNEL, "beta")
            .await
            .expect("writes");
        assert_eq!(read_channel(&db).await, "beta");
        let first = install_id(&db).await.expect("creates");
        let second = install_id(&db).await.expect("stable");
        assert_eq!(first, second);
    }
}
