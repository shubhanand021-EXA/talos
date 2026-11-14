import { createHash } from "node:crypto";
import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";

/** SHA-256 hex helper (matches the Rust mock parity scheme). */
export function sha256HexBytes(bytes: Uint8Array): string {
  return `sha256:${createHash("sha256").update(bytes).digest("hex")}`;
}

/** Maximum stored artifact size (5 MiB); larger captures hash but don't persist. */
export const MAX_ARTIFACT_BYTES = 5 * 1024 * 1024;

/** Count password fields in page HTML (case-insensitive).
 *
 * Two-step scan (CodeQL js/polynomial-redos #11): tag extraction uses a
 * single negated-class pass (`<input[^>]*>?`, linear — no nested
 * quantifiers), then each candidate tag is tested for a password type.
 * The previous single-regex scan nested `[^>]*` against `\s*`, which
 * backtracks polynomially on `<input` prefixes from untrusted pages (the
 * `live.ts` screenshot-redaction path feeds raw page HTML here).
 * Count semantics are unchanged for all inputs without `>` inside quoted
 * attribute values (malformed-HTML edge: such tags now scan to the first
 * `>`, matching fail-closed redaction counting).
 */
export function countPasswordFields(html: string): number {
  const tags = html.match(/<input[^>]*>?/gi) ?? [];
  return tags.filter((tag) => /\btype\s*=\s*["']?password["']?/i.test(tag))
    .length;
}

/** Result of persisting one capture artifact. */
export interface ArtifactRef {
  /** Content hash (`sha256:`). */
  hash: string;
  /** Absolute path when persisted, absent when skipped. */
  path?: string | undefined;
  /** Why persistence was skipped. */
  skipped?: "too_large" | "sensitive_page" | undefined;
}

/**
 * Persist a capture under `<profileDir>/artifacts/<sha256>.<ext>` when it fits
 * the budget and the page is not sensitive. Hashing always happens; sensitive
 * pages skip storage (hash-only receipt).
 */
export async function storeArtifact(
  profileDir: string,
  bytes: Uint8Array,
  ext: string,
  options?: { sensitive?: boolean | undefined },
): Promise<ArtifactRef> {
  const hash = sha256HexBytes(bytes);
  if (options?.sensitive === true) {
    return { hash, skipped: "sensitive_page" };
  }
  if (bytes.length > MAX_ARTIFACT_BYTES) {
    return { hash, skipped: "too_large" };
  }
  const dir = join(profileDir, "artifacts");
  await mkdir(dir, { recursive: true });
  const path = join(dir, `${hash.replace(/^sha256:/, "")}.${ext}`);
  await writeFile(path, bytes);
  return { hash, path };
}
