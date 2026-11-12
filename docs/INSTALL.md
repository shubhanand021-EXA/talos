# Installation

> Current status: the installer _pipeline_ is built and verified (bundler
> configs, offline signing, manifest verification, CLI sidecar staging), but
> no public release channel exists yet — there is nothing to download.
> Filenames below are the planned artifacts, not links.

## Planned artifacts (no downloads yet)

| OS      | Installer                    | Notes                                             |
| ------- | ---------------------------- | ------------------------------------------------- |
| Windows | `Talos_0.4.0_x64_en-US.msi`  | Per-machine NSIS alternative: `.exe`              |
| macOS   | `Talos_0.4.0_aarch64.dmg`    | Ad-hoc signed dev builds; notarized in production |
| Linux   | `Talos_0.4.0_amd64.AppImage` | Portable, no install; `.deb` for Debian/Ubuntu    |

Each installer bundles the desktop app, the `talos` CLI sidecar (release
builds), `LICENSE`, `README.md`, and icon assets. Installer size target:
**< 50 MB** (Tauri advantage over Electron — no bundled Chromium).

## Per-OS notes

### Windows (`.msi` + NSIS `.exe`)

- Requires WebView2 (evergreen). Dev bundles set
  `bundle.windows.webviewInstallMode: { "type": "skip" }`; release MSI/NSIS
  embeds or downloads the bootstrapper (see `tauri.conf.json`).
- **Development (unsigned):** build with `pnpm build:desktop`. Windows
  SmartScreen will warn — expected for unsigned dev builds.
- **Development (self-signed):** create a throwaway cert (NEVER the release
  key) and sign the installer for local testing:
  ```powershell
  $cert = New-SelfSignedCertificate -Type CodeSigningCert -Subject "CN=Talos Dev"
  Export-PfxCertificate $cert C:\temp\talos-dev.pfx -Password (ConvertTo-SecureString -String "dev" -Force -AsPlainText)
  signtool sign /fd SHA256 /f C:\temp\talos-dev.pfx /p dev /tr http://timestamp.digic Lee /td SHA256 Talos_0.4.0_x64_en-US.msi
  ```
- **Production:** purchase a code-signing certificate, store the PFX + PFX
  password in CI secrets, and inject `bundle.windows.certificateThumbprint`
  - `timestampUrl` at release time (e.g. `TAURI_WINDOWS_CERTIFICATE_THUMBPRINT`).
    Cross-platform signing from Linux/macOS CI uses `osslsigncode` via
    `bundle.windows.signCommand` (`%1` = binary placeholder).

### macOS (`.dmg`)

- Dev builds use ad-hoc signing (`bundle.macOS.signingIdentity: "-"`) with
  the hardened runtime — installs with a right-click → Open on first run.
- **Production:** enroll in the Apple Developer program, set a real
  `signingIdentity` ("Developer ID Application: …"), and add notarization
  (`APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` secrets + `tauri signer`
  / `notarytool` stapling) before publishing the `.dmg` and the updater
  artifacts.

### Linux (`.AppImage` + `.deb`)

- The `.AppImage` runs anywhere (no install, no root).
- The `.deb` declares `libwebkit2gtk-4.1-0` + `libssl3` dependencies and
  installs the desktop entry + icons for Debian/Ubuntu.
- No code signing on Linux; integrity comes from the ed25519-signed
  updater artifacts (same key as all platforms).

## CLI sidecar staging (release only)

Dev bundles intentionally omit the CLI so `tauri build` works without a
staged binary. Release automation stages it:

```sh
cargo build --release -p talos_cli --target <triple>   # per OS/arch
node scripts/stage-cli-binaries.mjs                    # → src-tauri/binaries/talos-<triple>[.exe]
node scripts/stage-cli-binaries.mjs --check            # release gate: exits 1 when empty
```

Then uncomment `bundle.externalBin` in `tauri.conf.json` and build. The
sidecar convention (`talos-<target-triple>`) lets the desktop spawn the
exact CLI it shipped with; users also get `talos` on PATH via the
installer layout (see `docs/CLI.md` for the headless contract).

## Data + CLI sharing one database

Desktop and CLI share the same file-backed `EvidenceDb` (see `docs/CLI.md`
§ "Desktop IPC for CLI integration"):

- `TALOS_DATA_DIR` unset → platform `com.talos.desktop` app-data dir
  (Windows `%APPDATA%`, macOS `~/Library/Application Support`, Linux
  `~/.local/share`; `TALOS_DATA_DIR` overrides for portable/CI runs).
- With at-rest encryption the CLI reuses the OS-keychain database key, so
  both surfaces read and write one ledger.
- Prefer CLI writes while the desktop is closed, or point `TALOS_DATA_DIR`
  at a scratch copy for fully isolated runs.
