set windows-shell := ["powershell.exe", "-NoLogo", "-Command"]

default:
    @just --list

# Install dependencies and verify tool versions
setup:
    pnpm install
    node --version
    pnpm --version
    rustc --version
    cargo --version
    git --version

# Run lint across repo (prettier + workspace lints if present)
lint:
    pnpm lint

# Format repo
format:
    pnpm format

# Strict TypeScript check
typecheck:
    pnpm typecheck

# Run tests (root smoke + workspace tests if present)
test:
    pnpm test

# Build workspaces if present
build:
    pnpm build

# Security / dependency audit
audit:
    pnpm audit

# Launch the Tauri desktop shell (Vite dev + native window)
dev-desktop:
    pnpm dev:desktop

# Bundle the desktop app (requires WebView2 + platform bundler deps)
build-desktop:
    pnpm build:desktop

# Print environment readiness
doctor:
    @echo "=== Talos doctor ==="
    @echo "--- versions ---"
    git --version
    node --version
    pnpm --version
    rustc --version
    cargo --version
    just --version
    @echo "--- workspace ---"
    @echo "root: {{justfile_directory()}}"
    pnpm --version
    @echo "doctor: ok"
