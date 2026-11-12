#!/usr/bin/env bash
#
# Project Talos clean-machine bootstrap (macOS / Debian-Ubuntu).
# Idempotent, pinned, re-runnable. Mirrors scripts/bootstrap-clean-machine.ps1.
# No @latest, no unpinned installs.
#
set -euo pipefail

# ---- Pinned versions (verified 2026-11-12, Report 001) ----
PIN_NODE="26.13.1"        # satisfies .nvmrc "26"
PIN_PNPM="10.34.7"        # packageManager pnpm@10.34.7
PIN_RUST="1.99.1"         # README badge; rust-toolchain.toml
PIN_JUST="1.59.0"         # requires >= 1.30
PIN_GIT_MIN="2.40"
REPO="shubhanand021-EXA/talos"

step() { echo; echo "=== $1 ==="; }
have() { command -v "$1" >/dev/null 2>&1; }

# ---- A1. OS / shell ----
step "A1: Detect OS and shell"
uname -a
if [ -f /etc/os-release ]; then cat /etc/os-release; fi
sw_vers 2>/dev/null || true
echo "ARCH: $(uname -m)"
echo "SHELL: ${SHELL:-unknown} bash ${BASH_VERSION:-unknown}"

# ---- A2. Git + gh FIRST ----
step "A2: Install Git and GitHub CLI"
if ! have git; then
  if have apt-get; then sudo apt-get update && sudo apt-get install -y git;
  elif have brew; then brew install git;
  else echo "Install git >= ${PIN_GIT_MIN} manually, then re-run."; exit 1; fi
fi
if ! have gh; then
  if have apt-get; then
    sudo mkdir -p /usr/share/keyrings
    curl -fsSL https://cli.github.com/packages/githubcli-archive-keyring.gpg | sudo dd of=/usr/share/keyrings/githubcli-archive-keyring.gpg
    echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main" | sudo tee /etc/apt/sources.list.d/github-cli.list >/dev/null
    sudo apt-get update && sudo apt-get install -y gh
  elif have brew; then brew install gh;
  else echo "Install GitHub CLI manually, then re-run."; exit 1; fi
fi
git --version
gh --version

# ---- A3. Owner auth (manual, once) ----
step "A3: Verify gh auth (owner runs gh auth login if needed)"
if ! gh auth status; then
  echo "NOT AUTHENTICATED. Owner action required:"
  echo "  gh auth login   # authenticate as shubhanand021-EXA (scopes: repo, workflow)"
  exit 1
fi

# ---- A4. Clone ----
step "A4: Clone repository"
if [ ! -d ./talos/.git ]; then gh repo clone "$REPO"; else echo "talos/ already cloned, skipping."; fi
cd ./talos
git rev-parse HEAD
git status --short | head -5 || true

# ---- A5. Read repo pins (repo wins) ----
step "A5: Read repo pin files"
echo "--- .nvmrc ---"; cat .nvmrc
echo "--- packageManager ---"; node -p "require('./package.json').packageManager" 2>/dev/null || echo "(node not installed yet, see A6)"
echo "--- rust-toolchain.toml ---"; cat rust-toolchain.toml 2>/dev/null || echo "(absent)"
grep -o 'img.shields.io/badge/\(rust\|node\|pnpm\)[^)]*' README.md || true
head -15 justfile

# ---- A6. Pinned toolchain ----
step "A6: Install pinned toolchain"
if ! have node || [ "$(node --version 2>/dev/null)" != "v${PIN_NODE}" ]; then
  if have brew; then brew install "node@${PIN_NODE%.*}" 2>/dev/null || brew install node;
  else
    echo "Installing Node v${PIN_NODE} via nodejs.org tarball..."
    TMPD="$(mktemp -d)"; trap 'rm -rf "$TMPD"' EXIT
    UNAME_M="$(uname -m)"; case "$UNAME_M" in x86_64) NODE_ARCH=x64;; arm64|aarch64) NODE_ARCH=arm64;; *) echo "unsupported arch $UNAME_M"; exit 1;; esac
    UNAME_S="$(uname -s)"; case "$UNAME_S" in Darwin) NODE_OS=darwin;; Linux) NODE_OS=linux;; *) echo "unsupported os $UNAME_S"; exit 1;; esac
    curl -fsSL "https://nodejs.org/dist/v${PIN_NODE}/node-v${PIN_NODE}-${NODE_OS}-${NODE_ARCH}.tar.xz" -o "$TMPD/node.tar.xz"
    tar -xf "$TMPD/node.tar.xz" -C "$TMPD"
    sudo cp -r "$TMPD/node-v${PIN_NODE}-${NODE_OS}-${NODE_ARCH}/"* /usr/local/
  fi
fi
node --version
# corepack was removed from Node >= 25 distributions; pin pnpm via npm.
npm install -g "pnpm@${PIN_PNPM}"
if [ "$(pnpm --version)" != "$PIN_PNPM" ]; then echo "pnpm version mismatch"; exit 1; fi

if ! have rustup; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
  # shellcheck disable=SC1091
  . "$HOME/.cargo/env"
fi
export PATH="$HOME/.cargo/bin:$PATH"
rustup toolchain install "$PIN_RUST"
rustup default "$PIN_RUST"
rustup component add rustfmt clippy --toolchain "$PIN_RUST"
rustc --version; cargo --version; cargo fmt --version; cargo clippy --version

if ! have just; then
  if have brew; then brew install just;
  elif have cargo; then cargo install just --locked --version "$PIN_JUST";
  else echo "Install just >= 1.30 manually."; exit 1; fi
fi
just --version

# ---- A7. OS build prerequisites ----
step "A7: OS build prerequisites"
if [ "$(uname -s)" = "Linux" ]; then
  sudo apt-get update
  sudo apt-get install -y build-essential curl file patchelf pkg-config libssl-dev \
    libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev bubblewrap
elif [ "$(uname -s)" = "Darwin" ]; then
  xcode-select -p >/dev/null 2>&1 || xcode-select --install
  echo "WebKit is native on macOS; no WebView2 step needed."
fi

# ---- A8. Project dependencies ----
step "A8: Install project dependencies"
pnpm install --frozen-lockfile
if [ -n "$(git diff --name-only -- pnpm-lock.yaml)" ]; then echo "pnpm-lock.yaml changed during install"; exit 1; fi
echo "lockfile unchanged: OK"
(cd crates && cargo fetch)
cd ..

step "Bootstrap complete"
echo "Verify with: pnpm build, then pnpm typecheck, pnpm lint, pnpm test, cargo fmt --check, cargo clippy, cargo test --workspace, pnpm audit:swarm"
