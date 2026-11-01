//! Screenshot + vision loop: capture, hash, compare, verify.
//!
//! The observe → act → verify cycle: capture the screen before an action,
//! execute through the accessibility driver, capture again, and compare the
//! `sha256:` hashes. Verification reports whether anything changed and,
//! when an expected region is known, whether that region plausibly changed.
//! Image bytes never leave this module except as content-addressed artifacts.

use sha2::{Digest, Sha256};

use crate::model::BoundingBox;

/// One captured frame: PNG bytes, dimensions, and content hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Screenshot {
    /// PNG-encoded frame bytes.
    pub png: Vec<u8>,
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// `sha256:` hex of the PNG bytes.
    pub hash: String,
}

/// Outcome of comparing two frames.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameComparison {
    /// True when the hashes differ.
    pub changed: bool,
    /// Hash of the before frame.
    pub before_hash: String,
    /// Hash of the after frame.
    pub after_hash: String,
}

/// Verification verdict for one executed action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verification {
    /// Whether any pixel changed between before and after.
    pub changed: bool,
    /// Whether the expected region plausibly changed.
    ///
    /// Computed without pixel access: a global hash change while an action
    /// targeted a region counts as plausible; no change at all counts as
    /// implausible. `None` when no expected region was declared.
    pub region_plausible: Option<bool>,
    /// Emitted when verification fails (`ActionVerificationFailed`).
    pub verification_failed: bool,
    /// Human detail for receipts and events.
    pub detail: String,
}

/// Hash raw bytes as `sha256:<hex>` (screenshots, thumbnails, payloads).
#[must_use]
pub fn hash_bytes(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

/// Build a [`Screenshot`] record from PNG bytes and dimensions.
#[must_use]
pub fn screenshot_from_png(png: Vec<u8>, width: u32, height: u32) -> Screenshot {
    let hash = hash_bytes(&png);
    Screenshot {
        png,
        width,
        height,
        hash,
    }
}

/// Compare two frames by hash.
#[must_use]
pub fn compare_frames(before: &Screenshot, after: &Screenshot) -> FrameComparison {
    FrameComparison {
        changed: before.hash != after.hash,
        before_hash: before.hash.clone(),
        after_hash: after.hash.clone(),
    }
}

/// Decide the verification verdict for one action.
///
/// `expected_region` is the bounding box the action targeted (if any).
/// Without pixel access the loop reasons conservatively: a targeted action
/// with zero frame change fails verification; an untargeted action that
/// changed the frame passes (something happened, nothing was promised).
#[must_use]
pub fn verify_action(
    comparison: &FrameComparison,
    expected_region: Option<&BoundingBox>,
    action: &str,
) -> Verification {
    let changed = comparison.changed;
    let region_plausible = expected_region.map(|_| changed);
    let verification_failed = match expected_region {
        Some(_) => !changed,
        None => false,
    };
    let detail = match (expected_region, changed) {
        (Some(_), true) => format!("{action}: frame changed as targeted"),
        (Some(_), false) => format!("{action}: no frame change in targeted region"),
        (None, true) => format!("{action}: frame changed (no region promised)"),
        (None, false) => format!("{action}: frame unchanged (read-only or no-op)"),
    };
    Verification {
        changed,
        region_plausible,
        verification_failed,
        detail,
    }
}

/// Minimal base64 encoder (thumbnails; avoids another dependency).
#[must_use]
pub fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = u32::from(*chunk.get(1).unwrap_or(&0));
        let b2 = u32::from(*chunk.get(2).unwrap_or(&0));
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[((triple >> 18) & 63) as usize] as char);
        out.push(ALPHABET[((triple >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[((triple >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[(triple & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// Encode RGBA pixels as PNG bytes (feature-gated real path only).
#[cfg(feature = "real-os-control")]
#[must_use]
pub fn encode_png_rgba(width: u32, height: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    use std::io::Cursor;
    let image = image::RgbaImage::from_raw(width, height, rgba.to_vec())?;
    let dynamic = image::DynamicImage::ImageRgba8(image);
    let mut bytes = Vec::new();
    dynamic
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
        .ok()?;
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shot(tag: &str) -> Screenshot {
        screenshot_from_png(tag.as_bytes().to_vec(), 1440, 900)
    }

    #[test]
    fn hashes_are_content_addressed() {
        let first = shot("frame-a");
        let second = shot("frame-a");
        assert_eq!(first.hash, second.hash);
        assert_ne!(first.hash, shot("frame-b").hash);
        assert!(first.hash.starts_with("sha256:"));
    }

    #[test]
    fn targeted_action_with_no_change_fails_verification() {
        let before = shot("same");
        let after = shot("same");
        let comparison = compare_frames(&before, &after);
        assert!(!comparison.changed);
        let region = BoundingBox {
            x: 10,
            y: 10,
            width: 100,
            height: 40,
        };
        let verdict = verify_action(&comparison, Some(&region), "computer.click");
        assert!(verdict.verification_failed);
        assert_eq!(verdict.region_plausible, Some(false));
    }

    #[test]
    fn targeted_action_with_change_passes_verification() {
        let comparison = compare_frames(&shot("before"), &shot("after"));
        let region = BoundingBox {
            x: 10,
            y: 10,
            width: 100,
            height: 40,
        };
        let verdict = verify_action(&comparison, Some(&region), "computer.click");
        assert!(!verdict.verification_failed);
        assert_eq!(verdict.region_plausible, Some(true));
    }

    #[test]
    fn readonly_capture_never_fails_verification() {
        let comparison = compare_frames(&shot("same"), &shot("same"));
        let verdict = verify_action(&comparison, None, "computer.capture_screen");
        assert!(!verdict.verification_failed);
    }

    #[test]
    fn base64_round_trips_padding_edges() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
    }
}
