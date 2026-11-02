//! Plugin trust foundation: manifest parsing, permission review, signature
//! verification (ed25519), and evidence-backed approval records.
//!
//! A plugin (MCP server or skill pack) ships a manifest:
//! `{ name, version, author, capabilities, permissions, source_url, signature }`.
//! New manifests render as a permission-review dialog; enabling requires an
//! explicit approval recorded in the evidence ledger. Signed manifests verify
//! against a 32-byte ed25519 public key; unsigned manifests enable only with
//! explicit user acknowledgement.

/// Plugin manifest (stored alongside the plugin, parsed before install).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifest {
    /// Plugin name (`1..=128` chars).
    pub name: String,
    /// Semantic version (`1..=32` chars).
    pub version: String,
    /// Author identity (`1..=256` chars).
    pub author: String,
    /// Capability strings the plugin exercises (1..=32).
    pub capabilities: Vec<String>,
    /// Permission strings the plugin requests (rendered for review).
    pub permissions: Vec<String>,
    /// Source URL (optional, max 2048 chars).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
    /// Hex ed25519 signature over the canonical manifest bytes (optional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

impl PluginManifest {
    /// Parse and validate raw JSON.
    pub fn parse(raw: &str) -> Result<Self, crate::ParseError> {
        if raw.len() > 65_536 {
            return Err(crate::ParseError::new(
                "plugin",
                "manifest must be at most 64 KiB",
            ));
        }
        let manifest: Self = serde_json::from_str(raw)
            .map_err(|err| crate::ParseError::new("plugin", format!("manifest parse: {err}")))?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Validate field bounds.
    pub fn validate(&self) -> Result<(), crate::ParseError> {
        let fail = |message: String| crate::ParseError::new("plugin", message);
        if self.name.trim().is_empty() || self.name.len() > 128 {
            return Err(fail("name must be 1..=128 chars".to_owned()));
        }
        if self.version.trim().is_empty() || self.version.len() > 32 {
            return Err(fail("version must be 1..=32 chars".to_owned()));
        }
        if self.author.trim().is_empty() || self.author.len() > 256 {
            return Err(fail("author must be 1..=256 chars".to_owned()));
        }
        if self.capabilities.is_empty() || self.capabilities.len() > 32 {
            return Err(fail("capabilities must declare 1..=32 entries".to_owned()));
        }
        if self.permissions.len() > 32 {
            return Err(fail("permissions hold at most 32 entries".to_owned()));
        }
        for entry in self.capabilities.iter().chain(self.permissions.iter()) {
            if entry.trim().is_empty() || entry.len() > 128 {
                return Err(fail(
                    "capability/permission entries must be 1..=128 chars".to_owned(),
                ));
            }
        }
        if let Some(url) = &self.source_url {
            if url.len() > 2048 {
                return Err(fail("source_url must be at most 2048 chars".to_owned()));
            }
        }
        if let Some(signature) = &self.signature {
            if signature.len() != 128 || !signature.chars().all(|ch| ch.is_ascii_hexdigit()) {
                return Err(fail(
                    "signature must be 128 hex chars (64-byte ed25519)".to_owned(),
                ));
            }
        }
        Ok(())
    }

    /// Canonical bytes covered by the signature (name + version + author +
    /// sorted capabilities + sorted permissions, newline-joined).
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut capabilities = self.capabilities.clone();
        capabilities.sort();
        let mut permissions = self.permissions.clone();
        permissions.sort();
        format!(
            "{}\n{}\n{}\n{}\n{}",
            self.name,
            self.version,
            self.author,
            capabilities.join(","),
            permissions.join(",")
        )
        .into_bytes()
    }

    /// True when a signature is present.
    #[must_use]
    pub fn is_signed(&self) -> bool {
        self.signature.is_some()
    }
}

/// Trust state for one installed plugin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrustStatus {
    /// Manifest parsed, awaiting user approval.
    Pending,
    /// Approved by the user (evidence recorded).
    Trusted,
    /// Explicitly rejected.
    Rejected,
}

/// Permission review shown in the UI before enabling.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionReview {
    /// Plugin name + version.
    pub plugin: String,
    /// Requested permissions (sorted, verbatim).
    pub requested: Vec<String>,
    /// Declared capabilities (sorted, verbatim).
    pub capabilities: Vec<String>,
    /// True when no signature was present (louder warning).
    pub unsigned: bool,
    /// True when the signature verified against the given key.
    pub signature_valid: bool,
}

/// Build the review dialog payload for a manifest.
#[must_use]
pub fn review_manifest(manifest: &PluginManifest, signature_valid: bool) -> PermissionReview {
    let mut requested = manifest.permissions.clone();
    requested.sort();
    let mut capabilities = manifest.capabilities.clone();
    capabilities.sort();
    PermissionReview {
        plugin: format!("{} {}", manifest.name, manifest.version),
        requested,
        capabilities,
        unsigned: !manifest.is_signed(),
        signature_valid,
    }
}

/// Verify a manifest signature against a 32-byte ed25519 public key (hex).
pub fn verify_signature(
    manifest: &PluginManifest,
    public_key_hex: &str,
) -> Result<bool, crate::ParseError> {
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};
    let Some(signature_hex) = &manifest.signature else {
        return Ok(false);
    };
    let key_bytes = hex::decode(public_key_hex)
        .map_err(|_| crate::ParseError::new("plugin", "public key must be hex"))?;
    if key_bytes.len() != 32 {
        return Err(crate::ParseError::new(
            "plugin",
            "public key must be 32 bytes (64 hex chars)",
        ));
    }
    let mut key_array = [0u8; 32];
    key_array.copy_from_slice(&key_bytes);
    let key = VerifyingKey::from_bytes(&key_array)
        .map_err(|err| crate::ParseError::new("plugin", format!("bad public key: {err}")))?;
    let signature_bytes = hex::decode(signature_hex)
        .map_err(|_| crate::ParseError::new("plugin", "signature must be hex"))?;
    if signature_bytes.len() != 64 {
        return Err(crate::ParseError::new(
            "plugin",
            "signature must be 64 bytes",
        ));
    }
    let mut signature_array = [0u8; 64];
    signature_array.copy_from_slice(&signature_bytes);
    let signature = Signature::from_bytes(&signature_array);
    Ok(key.verify(&manifest.canonical_bytes(), &signature).is_ok())
}

/// Evidence payload recording a plugin approval (`plugin_approval` rows).
#[must_use]
pub fn approval_evidence(
    manifest: &PluginManifest,
    approved_by: &str,
    approved: bool,
    signature_valid: bool,
) -> serde_json::Value {
    serde_json::json!({
        "plugin": manifest.name,
        "version": manifest.version,
        "author": manifest.author,
        "capabilities": manifest.capabilities,
        "permissions": manifest.permissions,
        "approved": approved,
        "approved_by": approved_by,
        "unsigned": !manifest.is_signed(),
        "signature_valid": signature_valid,
        "recorded_at": talos_common::now_iso(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn manifest() -> PluginManifest {
        PluginManifest {
            name: "fs-pack".to_owned(),
            version: "0.1.0".to_owned(),
            author: "talos".to_owned(),
            capabilities: vec!["tools".to_owned()],
            permissions: vec!["fs.read".to_owned(), "fs.list".to_owned()],
            source_url: Some("https://example.com/fs-pack".to_owned()),
            signature: None,
        }
    }

    #[test]
    fn parses_and_reviews_unsigned_manifest() {
        let raw = serde_json::to_string(&manifest()).unwrap();
        let parsed = PluginManifest::parse(&raw).unwrap();
        assert!(!parsed.is_signed());
        let review = review_manifest(&parsed, false);
        assert!(review.unsigned);
        assert!(!review.signature_valid);
        assert_eq!(review.requested.len(), 2);
        // Approval recorded as evidence payload.
        let evidence = approval_evidence(&parsed, "user-1", true, false);
        assert_eq!(evidence["approved"], serde_json::json!(true));
        assert_eq!(evidence["unsigned"], serde_json::json!(true));
    }

    #[test]
    fn signed_manifest_verifies_ed25519() {
        let secret = [7u8; 32];
        let signing = SigningKey::from_bytes(&secret);
        let verifying = signing.verifying_key();
        let public_hex = hex::encode(verifying.to_bytes());
        let mut signed = manifest();
        let signature = signing.sign(&signed.canonical_bytes());
        signed.signature = Some(hex::encode(signature.to_bytes()));
        assert!(signed.is_signed());
        assert!(verify_signature(&signed, &public_hex).unwrap());
        // Wrong key fails closed.
        let other = SigningKey::from_bytes(&[9u8; 32]).verifying_key();
        assert!(!verify_signature(&signed, &hex::encode(other.to_bytes())).unwrap());
    }

    #[test]
    fn rejects_bad_manifests() {
        let mut bad = manifest();
        bad.capabilities.clear();
        assert!(bad.validate().is_err());
        bad = manifest();
        bad.signature = Some("zz".to_owned());
        assert!(bad.validate().is_err());
        assert!(PluginManifest::parse("{not json}").is_err());
    }
}
