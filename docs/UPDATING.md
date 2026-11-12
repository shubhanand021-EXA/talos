# Auto-update pipeline

> Status: the pipeline is built and covered by workspace tests
> (`crates/talos_updates`: manifest parsing, rollout gating, tamper/wrong-key
> rejection) and the signing flow is real (`scripts/sign-release.mjs`), but no
> update channel is live yet — the endpoint below answers nothing until the
> first release channel exists.

Signed, staged, channel-aware updates. Verification happens twice: the
release manifest carries a minisign signature over its canonical JSON, and
every installed bundle is ed25519-verified against the pubkey pinned in
`tauri.conf.json` before it touches the installation.

## Manifest format

```json
{
  "version": "0.5.0",
  "notes": "Hardening release",
  "pub_date": "2026-11-02T00:00:00Z",
  "rollout_percentage": 25,
  "artifacts": [
    {
      "target": "windows-x86_64",
      "arch": "x86_64",
      "url": "https://updates.talos.dev/stable/windows-x86_64/x86_64/0.4.0/Talos_0.5.0.msi",
      "signature": "<minisign .sig text for the bundle>"
    }
  ]
}
```

- `rollout_percentage` (0–100, optional): staged rollout gate. Each install
  hashes its stable `install_id` (persisted in shared `app_state`) into a
  0–99 bucket; only buckets below the percentage are offered the update.
  `None` = fully rolled out. Same install id → same verdict forever (no
  mid-rollout flapping). Pure functions in `crates/talos_updates`
  (`parse_update_manifest`, `rollout_applies`) — covered by workspace tests,
  including tamper/wrong-key/garbage rejection.
- `signature` (manifest-level): minisign `.sig` text over the canonical
  manifest JSON, produced offline by `scripts/sign-release.mjs sign`
  (private key via `--key <file>` or `$TALOS_UPDATER_PRIVATE_KEY` — never
  committed). Verified by `talos_updates::verify_manifest_signature`
  (same `minisign-verify` semantics the Tauri updater uses) and the
  `verify_update_manifest` IPC release automation calls before publishing.

## Update flow

a) **Startup / on demand:** `check_for_updates` (or the TopBar banner's
`get_update_status`) queries
`https://updates.talos.dev/{channel}/{{target}}/{{arch}}/{{current_version}}`
— availability only, never downloads.
b) **Download:** `download_update` fetches bundle bytes (signature-verified
by the plugin during transfer) and stages them in the OS temp dir,
recorded in `AppState::pending_update` (`staged` + `stagedVersion` in the
status view).
c) **Verify:** the updater plugin verifies the bundle signature against the
pinned pubkey; a bad signature aborts before anything is staged.
d) **Prompt:** the TopBar banner switches Download → Install & restart;
Settings → Updates shows the same state.
e) **Install:** `install_update` prefers staged bytes for the same version
(`Update::install`, still verified) and falls back to
`download_and_install` otherwise — then restarts.

`install_update` without a pending update behaves like the classic flow
(check → download → verify → install → restart).

## Channels

`stable` (default) · `beta` · `nightly`, persisted in shared `app_state`
(`get_update_channel` / `set_update_channel`; unknown values fall back to
stable so a corrupt preference never bricks updates). The endpoint template
(`UPDATE_ENDPOINT_TEMPLATE`) carries `{{channel}}`; beta/nightly builds
ship with their own endpoint — switching channel applies on restart.
The CLI and desktop agree on the channel through the same database row.

## Release checklist

1. Bump `version` (root `package.json`, `tauri.conf.json`, crate versions).
2. `pnpm build:desktop` per OS (with staged CLI sidecars — `docs/INSTALL.md`).
3. `node scripts/sign-release.mjs sign <bundle>` per artifact (offline key).
4. Publish manifest JSON + `.sig` per channel; smoke-verify with
   `verify_update_manifest` before pointing the endpoint at it.
5. Ramp `rollout_percentage`: 1 → 10 → 25 → 50 → 100, watching the
   `update_check_failed` diagnostics (endpoint is named in the error).
