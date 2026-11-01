import { createHash } from "node:crypto";
import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";

/** SHA-256 hex helper (matches the Rust mock parity scheme). */
export function sha256HexBytes(bytes: Uint8Array): string {
  return `sha256:${createHash("sha256").update(bytes).digest("hex")}`;
}

/** Maximum stored artifact size (5 MiB); larger captures hash but don't persist. */
export const MAX_ARTIFACT_BYTES = 5 * 1024 * 1024;

/** Count password fields in page HTML (case-insensitive). */
export function countPasswordFields(html: string): number {
  const matches = html.match(/<input[^>]*type\s*=\s*["']?password["']?/gi);
  return matches === null ? 0 : matches.length;
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
