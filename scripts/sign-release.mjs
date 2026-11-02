#!/usr/bin/env node
/**
 * Talos release signing (minisign-compatible ed25519, no dependencies).
 *
 * The desktop updater verifies bundles with the embedded ed25519 public key
 * (see `apps/desktop/src-tauri/tauri.conf.json` → `plugins.updater.pubkey`).
 * Release bundles MUST be signed with the matching private key.
 *
 *   node scripts/sign-release.mjs genkey --out <name>
 *     Writes <name>.key (UNENCRYPTED minisign secret key — protect it like a
 *     password) and <name>.pub, prints the pubkey for tauri.conf.json.
 *
 *   node scripts/sign-release.mjs sign <file> [--key <keyfile> | --key-env VAR]
 *     Writes <file>.sig (minisign signature file) and prints the manifest
 *     `signature` block. The private key MUST NEVER be committed to the repo:
 *     pass it via env (`TALOS_UPDATER_PRIVATE_KEY`, raw base64 of the secret
 *     key line) or a file outside the repo.
 *
 *   node scripts/sign-release.mjs verify <file> --signature <sigfile> --pubkey <pub|key-string>
 *     Verifies a bundle against its signature (used by CI smoke tests).
 *
 * Format notes (minisign-compatible subset, unencrypted keys only):
 * - .pub second line: base64("Ed" + keynum[8] + pubkey[32])
 * - .key second line: base64("Ed" + keynum[8] + seed[32] + pubkey[32])
 * - .sig: "untrusted comment" line, base64("Ed" + keynum[8] + filesig[64]),
 *   "trusted comment: timestamp:<unix>\tfile:<basename>" line, then the
 *   base64 global signature over (filesig || trusted-comment BODY, i.e.
 *   the text after the "trusted comment: " prefix — matching upstream
 *   minisign and the `minisign-verify` crate the Tauri updater uses, so
 *   release signatures verify end-to-end).
 */

import { readFileSync, writeFileSync } from "node:fs";
import { basename, dirname, join } from "node:path";
import { webcrypto } from "node:crypto";

const subtle = webcrypto.subtle;
const SIG_MAGIC = Buffer.from("Ed", "ascii");

function b64encode(bytes) {
  return Buffer.from(bytes).toString("base64");
}

function b64decode(text) {
  const raw = Buffer.from(text.trim(), "base64");
  if (raw.length === 0) throw new Error("invalid base64");
  return raw;
}

function parseArgs(argv) {
  const [command, ...rest] = argv;
  const flags = {};
  const positionals = [];
  for (let i = 0; i < rest.length; i += 1) {
    const token = rest[i];
    if (token.startsWith("--")) {
      const name = token.slice(2);
      const next = rest[i + 1];
      if (next !== undefined && !next.startsWith("--")) {
        flags[name] = next;
        i += 1;
      } else {
        flags[name] = true;
      }
    } else {
      positionals.push(token);
    }
  }
  return { command, flags, positionals };
}

async function genkey(outBase) {
  const pair = await subtle.generateKey({ name: "Ed25519" }, true, [
    "sign",
    "verify",
  ]);
  const pubRaw = Buffer.from(await subtle.exportKey("raw", pair.publicKey));
  const pkcs8 = Buffer.from(await subtle.exportKey("pkcs8", pair.privateKey));
  // RFC 8410 OneAsymmetricKey: the 32-byte seed is the PKCS#8 tail.
  const seed = pkcs8.subarray(pkcs8.length - 32);
  const keynum = webcrypto.getRandomValues(new Uint8Array(8));
  const pubLine = b64encode(
    Buffer.concat([SIG_MAGIC, Buffer.from(keynum), pubRaw]),
  );
  const secLine = b64encode(
    Buffer.concat([SIG_MAGIC, Buffer.from(keynum), seed, pubRaw]),
  );
  const keyId = Buffer.from(keynum).toString("hex").toUpperCase();
  writeFileSync(
    `${outBase}.pub`,
    `untrusted comment: talos updater public key: ${keyId}\n${pubLine}\n`,
  );
  writeFileSync(
    `${outBase}.key`,
    `untrusted comment: TALOS UPDATER PRIVATE KEY — DO NOT COMMIT\n${secLine}\n`,
    { mode: 0o600 },
  );
  try {
    await import("node:fs").then(({ chmodSync }) =>
      chmodSync(`${outBase}.key`, 0o600),
    );
  } catch {
    // Best effort: Windows ACLs already restrict user files.
  }
  console.log(`public key written to ${outBase}.pub`);
  console.log(
    `private key written to ${outBase}.key (KEEP SECRET, NEVER COMMIT)`,
  );
  console.log(`tauri.conf.json pubkey:\n${pubLine}`);
  return { pubLine, keyFile: `${outBase}.key`, pubFile: `${outBase}.pub` };
}

function parseSecretKey(text) {
  const lines = text.trim().split("\n");
  if (lines.length < 2) throw new Error("malformed secret key file");
  const raw = b64decode(lines[lines.length - 1]);
  if (raw.length !== 2 + 8 + 64 || raw.subarray(0, 2).toString() !== "Ed") {
    throw new Error("malformed secret key payload (want 74 bytes, Ed magic)");
  }
  return {
    keynum: raw.subarray(2, 10),
    seed: raw.subarray(10, 42),
    pubkey: raw.subarray(42, 74),
  };
}

function parsePublicKey(text) {
  const lines = text.trim().split("\n");
  const raw = b64decode(lines[lines.length - 1]);
  if (raw.length !== 2 + 8 + 32 || raw.subarray(0, 2).toString() !== "Ed") {
    throw new Error("malformed public key payload (want 42 bytes, Ed magic)");
  }
  return { keynum: raw.subarray(2, 10), pubkey: raw.subarray(10, 42) };
}

async function importPrivate(seed) {
  // Rebuild the PKCS#8 envelope around the raw seed for WebCrypto import.
  const header = Buffer.from("302e020100300506032b657004220420", "hex");
  const pkcs8 = Buffer.concat([header, Buffer.from(seed)]);
  return subtle.importKey("pkcs8", pkcs8, { name: "Ed25519" }, false, ["sign"]);
}

async function signFile(filePath, secretKeyText) {
  const { keynum, seed } = parseSecretKey(secretKeyText);
  const data = readFileSync(filePath);
  const privateKey = await importPrivate(seed);
  const fileSig = Buffer.from(
    await subtle.sign({ name: "Ed25519" }, privateKey, data),
  );
  const sigLine = b64encode(
    Buffer.concat([SIG_MAGIC, Buffer.from(keynum), fileSig]),
  );
  const trustedComment = `trusted comment: timestamp:${Math.floor(Date.now() / 1000)}\tfile=${basename(filePath)}`;
  // Global signature covers the file signature concatenated with the
  // trusted-comment BODY (after the "trusted comment: " prefix), matching
  // upstream minisign and `minisign-verify` semantics: global = sign(
  // filesig || trusted_comment_body ). The Tauri updater plugin verifies
  // with the same construction, so release signatures verify end-to-end.
  const trustedCommentBody = trustedComment.slice("trusted comment: ".length);
  const globalSig = Buffer.from(
    await subtle.sign(
      { name: "Ed25519" },
      privateKey,
      Buffer.concat([fileSig, Buffer.from(trustedCommentBody, "utf8")]),
    ),
  );
  const sigText = [
    "untrusted comment: signature from talos release key",
    sigLine,
    trustedComment,
    b64encode(globalSig),
    "",
  ].join("\n");
  writeFileSync(`${filePath}.sig`, sigText);
  return { sigPath: `${filePath}.sig`, sigText };
}

async function importPublic(pubBytes) {
  return subtle.importKey("raw", pubBytes, { name: "Ed25519" }, false, [
    "verify",
  ]);
}

function parseSignatureFile(text) {
  const lines = text.trim().split("\n");
  if (lines.length < 4) throw new Error("malformed signature file");
  const blob = b64decode(lines[1]);
  if (blob.length !== 2 + 8 + 64 || blob.subarray(0, 2).toString() !== "Ed") {
    throw new Error("malformed signature blob (want 74 bytes, Ed magic)");
  }
  return {
    keynum: blob.subarray(2, 10),
    fileSig: blob.subarray(10, 74),
    trustedComment: lines[2],
    globalSig: b64decode(lines[3]),
  };
}

async function verifyFile(filePath, sigText, pubText) {
  const { keynum, pubkey } = parsePublicKey(pubText);
  const {
    keynum: sigKeynum,
    fileSig,
    trustedComment,
    globalSig,
  } = parseSignatureFile(sigText);
  if (!Buffer.from(keynum).equals(Buffer.from(sigKeynum))) {
    throw new Error("signature key id does not match public key");
  }
  const data = readFileSync(filePath);
  const publicKey = await importPublic(pubkey);
  const fileOk = await subtle.verify(
    { name: "Ed25519" },
    publicKey,
    fileSig,
    data,
  );
  if (!fileOk) throw new Error("file signature INVALID");
  // Global signature covers (filesig || comment body) — see `signFile`.
  const trustedCommentBody = trustedComment.slice("trusted comment: ".length);
  const globalOk = await subtle.verify(
    { name: "Ed25519" },
    publicKey,
    globalSig,
    Buffer.concat([
      Buffer.from(fileSig),
      Buffer.from(trustedCommentBody, "utf8"),
    ]),
  );
  if (!globalOk) throw new Error("trusted-comment signature INVALID");
  return true;
}

async function resolveSecretKey(flags) {
  if (flags.key) return readFileSync(flags.key, "utf8");
  const envName = flags["key-env"] ?? "TALOS_UPDATER_PRIVATE_KEY";
  if (process.env[envName]) {
    return `untrusted comment: private key from env ${envName}\n${process.env[envName].trim()}\n`;
  }
  throw new Error(
    `no private key: pass --key <keyfile> or set ${envName} (NEVER commit the key)`,
  );
}

async function main() {
  const { command, flags, positionals } = parseArgs(process.argv.slice(2));
  if (command === "genkey") {
    const out = flags.out ?? join(dirname("."), "talos-updater");
    await genkey(positionals[0] ?? out);
    return;
  }
  if (command === "sign") {
    const [file] = positionals;
    if (!file)
      throw new Error("usage: sign <file> [--key <keyfile> | --key-env VAR]");
    const secret = await resolveSecretKey(flags);
    const { sigPath, sigText } = await signFile(file, secret);
    console.log(`signature written to ${sigPath}`);
    console.log("manifest `signature` block (paste verbatim):");
    console.log(sigText.trimEnd());
    return;
  }
  if (command === "verify") {
    const [file] = positionals;
    if (!file || !flags.signature || !flags.pubkey) {
      throw new Error(
        "usage: verify <file> --signature <sigfile> --pubkey <pubfile|key-string>",
      );
    }
    let pubText = flags.pubkey;
    try {
      pubText = readFileSync(flags.pubkey, "utf8");
    } catch {
      // Treat as a literal key string.
    }
    await verifyFile(file, readFileSync(flags.signature, "utf8"), pubText);
    console.log(`OK: ${file} signature valid`);
    return;
  }
  throw new Error("usage: sign-release.mjs <genkey|sign|verify> [...]");
}

main().catch((err) => {
  console.error(`sign-release: ${String(err?.message ?? err)}`);
  process.exit(1);
});
