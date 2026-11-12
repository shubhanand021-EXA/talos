# Third-Party Notices — Project Talos

Talos itself is Apache-2.0 (see [LICENSE](LICENSE),
[NOTICE](NOTICE)). This file states the dependency license posture;
per-package license texts ship inside their own packages and manifests.

## Posture (enforced, not aspirational)

- Every dependency in the tree must carry a license on the audit allowlist
  (`.talos/audit.config.json` → `allowedLicenses`): MIT, Apache-2.0, ISC,
  BSD-2-Clause, BSD-3-Clause, CC0-1.0, Unlicense, 0BSD, Python-2.0,
  MPL-2.0, MIT-0, Apache-2.0 OR MIT, CC-BY-4.0, BlueOak-1.0.0, Zlib.
- No GPL/AGPL/LGPL copyleft code is admitted to the tree.
- Enforcement: the audit swarm dependencies agent (`tools/audit`,
  `license-allowlist` check over `pnpm licenses list`) runs on every CI
  `audit` job and fails the run on non-allowlisted licenses
  (see `docs/CI.md`).
- Reproduce the full inventory any time: `pnpm licenses list --json`
  (JS tree). Rust crates declare `license = "Apache-2.0"` (workspace root
  `crates/Cargo.toml`, inherited by all members); registry license texts
  for each crate version ship on crates.io.

## History

- Until Prompt 004 the workspace was MIT ("Copyright (c) 2026 Talos
  Contributors"); the MIT→Apache-2.0 relicense landed as a forward commit
  per `docs/adr/0013-apache2-relicense.md`. History was not rewritten.
