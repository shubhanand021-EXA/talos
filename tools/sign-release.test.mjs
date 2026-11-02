import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const script = join(root, "scripts", "sign-release.mjs");

function run(args, options = {}) {
  return execFileSync("node", [script, ...args], {
    encoding: "utf8",
    ...options,
  });
}

describe("sign-release", () => {
  it("round-trips genkey, sign, and verify (tampering fails)", () => {
    const dir = mkdtempSync(join(tmpdir(), "talos-sign-"));
    try {
      const out = run(["genkey", "--out", join(dir, "release")]);
      assert.match(out, /tauri\.conf\.json pubkey/);
      const bundle = join(dir, "bundle.bin");
      writeFileSync(bundle, Buffer.from("talos-test-bundle-payload"));
      run(["sign", bundle, "--key", join(dir, "release.key")]);
      const sig = readFileSync(`${bundle}.sig`, "utf8");
      assert.match(sig, /^untrusted comment:/m);
      assert.match(sig, /^trusted comment: timestamp:\d+\tfile=bundle\.bin$/m);
      const ok = run([
        "verify",
        bundle,
        "--signature",
        `${bundle}.sig`,
        "--pubkey",
        join(dir, "release.pub"),
      ]);
      assert.match(ok, /signature valid/);
      // Tampered bytes must not verify.
      writeFileSync(bundle, Buffer.from("talos-test-bundle-PAYLOAD"));
      assert.throws(
        () =>
          run([
            "verify",
            bundle,
            "--signature",
            `${bundle}.sig`,
            "--pubkey",
            join(dir, "release.pub"),
          ]),
        /INVALID/,
      );
      // A different key must not verify either.
      run(["genkey", "--out", join(dir, "other")]);
      assert.throws(
        () =>
          run([
            "verify",
            bundle,
            "--signature",
            `${bundle}.sig`,
            "--pubkey",
            join(dir, "other.pub"),
          ]),
        /does not match|INVALID/,
      );
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("accepts the private key from an env var", () => {
    const dir = mkdtempSync(join(tmpdir(), "talos-sign-env-"));
    try {
      run(["genkey", "--out", join(dir, "release")]);
      const keyText = readFileSync(join(dir, "release.key"), "utf8");
      const secretLine = keyText.trim().split("\n").at(-1);
      const bundle = join(dir, "bundle.bin");
      writeFileSync(bundle, Buffer.from("env-key-bundle"));
      const env = {
        ...process.env,
        TALOS_UPDATER_PRIVATE_KEY_TEST: secretLine,
      };
      run(["sign", bundle, "--key-env", "TALOS_UPDATER_PRIVATE_KEY_TEST"], {
        env,
      });
      const ok = run(
        [
          "verify",
          bundle,
          "--signature",
          `${bundle}.sig`,
          "--pubkey",
          join(dir, "release.pub"),
        ],
        { env },
      );
      assert.match(ok, /signature valid/);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });
});
