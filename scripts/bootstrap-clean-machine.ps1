#Requires -Version 5.1
<#
.SYNOPSIS
  Project Talos clean-machine bootstrap (Windows).
  Idempotent, pinned, re-runnable. Reproduces Report 001 Phase A exactly.
.DESCRIPTION
  Installs/verifies: Git, GitHub CLI, Node LTS, pnpm, Rust, just, Windows
  build prerequisites, then clones talos (if missing) and installs
  project dependencies. No @latest, no unpinned installs.
  Run from an empty parent workspace. The repo is created as .\talos.
#>
$ErrorActionPreference = 'Stop'

# ---- Pinned versions (verified 2026-11-12, Report 001) ----
$PIN_GIT_WINGET        = '2.56.0.4'    # git version 2.56.0.windows.4
$PIN_GH_WINGET         = '2.103.3'
$PIN_NODE_WINGET       = '26.13.1'     # satisfies .nvmrc "26"
$PIN_PNPM_NPM          = '10.34.7'     # packageManager pnpm@10.34.7
$PIN_RUSTUP_WINGET     = '1.29.3'
$PIN_RUST_TOOLCHAIN    = '1.99.1'      # README badge; rust-toolchain.toml
$PIN_JUST_WINGET       = '1.59.0'      # requires >= 1.30
$PIN_NSIS_WINGET       = '3.13'
$PIN_SDK_WINGET        = '10.0.26100.7705'
$PIN_BUILDTOOLS_WINGET = '17.14.45'
$REPO                 = 'shubhanand021-EXA/talos'

function Write-Step($msg) { Write-Host "`n=== $msg ===" -ForegroundColor Cyan }
function Refresh-Path {
  $env:Path = [System.Environment]::GetEnvironmentVariable('Path','Machine') + ';' +
              [System.Environment]::GetEnvironmentVariable('Path','User')
}
function Test-Cmd($name) { $null -ne (Get-Command $name -ErrorAction SilentlyContinue) }

# ---- A1. OS / shell ----
Write-Step 'A1: Detect OS and shell'
$os = Get-ComputerInfo -Property OsName,OsVersion,OsArchitecture,WindowsVersion | Out-String
Write-Host $os
Write-Host "ARCH: $env:PROCESSOR_ARCHITECTURE"
Write-Host "PowerShell: $($PSVersionTable.PSVersion)"
winget --version
Refresh-Path

# ---- A2. Git + gh FIRST ----
Write-Step 'A2: Install Git and GitHub CLI (winget, pinned)'
if (-not (Test-Cmd git)) {
  winget install --id Git.Git -e --version $PIN_GIT_WINGET --silent --accept-package-agreements --accept-source-agreements
} else { Write-Host 'git already present, skipping install.' }
if (-not (Test-Cmd gh)) {
  winget install --id GitHub.cli -e --version $PIN_GH_WINGET --silent --accept-package-agreements --accept-source-agreements
} else { Write-Host 'gh already present, skipping install.' }
Refresh-Path
git --version
if ($LASTEXITCODE -ne 0 -or -not (git --version | Select-String 'git version')) { throw 'git verification failed' }
gh --version

# ---- A3. Owner auth (manual, once) ----
Write-Step 'A3: Verify gh auth (owner runs gh auth login if needed)'
Refresh-Path
gh auth status
if ($LASTEXITCODE -ne 0) {
  Write-Host 'NOT AUTHENTICATED. Owner action required:' -ForegroundColor Yellow
  Write-Host '  gh auth login   # authenticate as shubhanand021-EXA (scopes: repo, workflow)'
  throw 'gh auth missing. Re-run this script after owner login.'
}

# ---- A4. Clone ----
Write-Step 'A4: Clone repository'
if (-not (Test-Path './talos/.git')) {
  gh repo clone $REPO
} else { Write-Host 'talos/ already cloned, skipping.' }
Set-Location './talos'
git rev-parse HEAD
git status --short | Select-Object -First 5

# ---- A5. Read repo pins (repo wins) ----
Write-Step 'A5: Read repo pin files'
Write-Host "--- .nvmrc ---"; Get-Content .nvmrc
Write-Host "--- packageManager ---"; node -p "require('./package.json').packageManager"
Write-Host "--- rust-toolchain.toml ---"; Get-Content rust-toolchain.toml -ErrorAction SilentlyContinue
Write-Host '--- README toolchain badges ---'
Select-String -Path README.md -Pattern 'img.shields.io/badge/(rust|node|pnpm)' | ForEach-Object { $_.Line.Trim() }
Write-Host "--- justfile (first 15) ---"; Get-Content justfile | Select-Object -First 15

# ---- A6. Pinned toolchain ----
Write-Step 'A6: Install pinned toolchain'
if (-not (Test-Cmd node)) {
  winget install --id OpenJS.NodeJS.LTS -e --version $PIN_NODE_WINGET --silent --accept-package-agreements --accept-source-agreements
} else { Write-Host 'node already present, skipping install.' }
Refresh-Path
node --version
npm install -g "pnpm@$PIN_PNPM_NPM"
pnpm --version
if ((pnpm --version) -ne $PIN_PNPM_NPM) { throw "pnpm version mismatch: expected $PIN_PNPM_NPM" }

if (-not (Test-Cmd rustup)) {
  winget install --id Rustlang.Rustup -e --version $PIN_RUSTUP_WINGET --silent --accept-package-agreements --accept-source-agreements
} else { Write-Host 'rustup already present, skipping install.' }
Refresh-Path
$env:Path += ';' + "$env:USERPROFILE\.cargo\bin"
rustup toolchain install $PIN_RUST_TOOLCHAIN
rustup default $PIN_RUST_TOOLCHAIN
rustup component add rustfmt clippy --toolchain $PIN_RUST_TOOLCHAIN
rustc --version; cargo --version; cargo fmt --version; cargo clippy --version

if (-not (Test-Cmd just)) {
  winget install --id Casey.Just -e --version $PIN_JUST_WINGET --silent --accept-package-agreements --accept-source-agreements
} else { Write-Host 'just already present, skipping install.' }
Refresh-Path
just --version

# ---- A7. Windows build prerequisites ----
Write-Step 'A7: Windows build prerequisites'
$webview = Get-ItemProperty -Path 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\*' -ErrorAction SilentlyContinue |
  Where-Object { $_.name -like '*WebView*' } | Select-Object -First 1
if ($webview) { Write-Host "WebView2 present: $($webview.name) $($webview.pv)" }
else {
  Write-Host 'Installing WebView2 Runtime...'
  winget install --id Microsoft.EdgeWebView2Runtime -e --silent --accept-package-agreements --accept-source-agreements
}
if (-not (Test-Cmd makensis)) {
  winget install --id NSIS.NSIS -e --version $PIN_NSIS_WINGET --silent --accept-package-agreements --accept-source-agreements
} else { Write-Host 'NSIS already present, skipping install.' }
Refresh-Path
if (Test-Cmd makensis) { makensis -VERSION } else { Write-Host 'NSIS not on PATH yet (new shell may be needed).' }
$linkFound = $null -ne (Get-ChildItem -Path 'C:\Program Files (x86)\Microsoft Visual Studio' -Recurse -Filter 'link.exe' -ErrorAction SilentlyContinue | Select-Object -First 1)
if (-not $linkFound) {
  Write-Host 'Installing Windows SDK + MSVC Build Tools (large, one-time)...'
  winget install --id Microsoft.WindowsSDK.10.0.26100 -e --version $PIN_SDK_WINGET --silent --accept-package-agreements --accept-source-agreements
  winget install --id Microsoft.VisualStudio.2022.BuildTools -e --version $PIN_BUILDTOOLS_WINGET --silent --accept-package-agreements --accept-source-agreements --override "--wait --quiet --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
} else { Write-Host 'MSVC link.exe already present, skipping Build Tools install.' }

# ---- A8. Project dependencies ----
Write-Step 'A8: Install project dependencies'
pnpm install --frozen-lockfile
git diff --name-only -- pnpm-lock.yaml
if (git diff --name-only -- pnpm-lock.yaml) { throw 'pnpm-lock.yaml changed during install' }
Write-Host 'lockfile unchanged: OK'
Set-Location './crates'
cargo fetch
Set-Location '..'

Write-Step 'Bootstrap complete'
Write-Host 'Verify with: pnpm build, then pnpm typecheck, pnpm lint, pnpm test, cargo fmt --check, cargo clippy, cargo test --workspace, pnpm audit:swarm'
