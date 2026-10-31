# CI workflows

## `ci.yml` (required)

Triggers: `push` to `main`, `pull_request` to `main`, `workflow_dispatch`, nightly `schedule`. Docs-only changes (`docs/**`, `*.md`) are filtered via `paths-ignore`.

Concurrency: `ci-${{ github.ref }}` with cancel-in-progress (latest run wins per ref). Timeouts per job; artifacts retained 14 days.

Caching:

- `typescript` + `audit`: `pnpm/action-setup@10.34.7` + `actions/setup-node@26` with `cache: pnpm` (keyed on `pnpm-lock.yaml`).
- `rust-core` + `desktop-check`: `Swatinem/rust-cache@v2` over `crates -> target` and `apps/desktop/src-tauri -> target`.

Jobs:

| Job             | What it proves                                                                  | Key commands                                                                                                                                         |
| --------------- | ------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| `meta`          | Enterprise storefront files + toolchain pins exist                              | `test -f` over README/LICENSE/CONTRIBUTING/SECURITY/CODE_OF_CONDUCT/CHANGELOG/SUPPORT/CODEOWNERS/templates/media; prints `.nvmrc` + `packageManager` |
| `typescript`    | JS/TS is lint-clean, strictly typed, tested, buildable                          | `pnpm install --frozen-lockfile`, `pnpm lint`, `pnpm typecheck`, `pnpm test`, `pnpm build`                                                           |
| `rust-core`     | Core crates are formatted, warning-free, tested                                 | `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (in `crates/`)                       |
| `desktop-check` | Tauri shell compiles on Linux (no bundle)                                       | apt WebKit2GTK prerequisites, `cargo fmt --check`, `cargo clippy -D warnings`, `cargo check --all-targets` (in `apps/desktop/src-tauri`)             |
| `audit`         | Dependencies + 9-agent swarm green                                              | `pnpm audit --audit-level high`, `pnpm audit:swarm`; uploads `docs/reports/` as `talos-audit-reports`                                                |
| `media-check`   | Brand assets exist, README refs resolve, GIFs valid, regeneration deterministic | `test -f docs/media/*`, README `docs/media/*` ref check, `GIF89a` header check, `node scripts/media/generate-brand.mjs` + clean `git status`         |

Honest isolation:

- Full Tauri bundle (`pnpm build:desktop`) is **not** in CI — needs platform bundlers (NSIS/WiX/appimagetool), signing identity, and a display. Covered locally per `docs/RUNBOOK.md`.
- No flaky GUI, network, or LLM steps in CI. If a job cannot be stable, it must be marked `continue-on-error` with a tracking issue — currently no jobs are marked unstable; all six are required green.

## Reading status

- Repo home badges → click **CI** for the latest `main` run.
- PRs: all six jobs must pass before merge. `audit` uploads `talos-audit-reports` even on failure (`if: always()`).
- Rerun: Actions tab → failed run → **Re-run failed jobs**. For `audit` failures, open `docs/reports/audit-latest.md`, fix `critical` rows, re-run `pnpm audit:swarm` locally, commit updated reports if they changed.

## Local parity

```sh
pnpm install --frozen-lockfile
pnpm lint && pnpm typecheck && pnpm test && pnpm build
pnpm audit && pnpm audit:swarm
node scripts/media/generate-brand.mjs && git status --porcelain -- docs/media
cd crates && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
cd ../../apps/desktop/src-tauri && cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings && cargo check --all-targets
```

> This repository is public (see [ADR 0005](../../docs/adr/0005-public-repository-transition.md)): badges (`.../actions/workflows/ci.yml/badge.svg`) and report links render for everyone.
