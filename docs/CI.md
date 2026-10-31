# Talos CI

CI workflow: [`.github/workflows/ci.yml`](/.github/workflows/ci.yml) (see also [`.github/workflows/README.md`](/.github/workflows/README.md)).

## What runs

Triggers: `push` to `main`, `pull_request` to `main`, `workflow_dispatch`.

| Job             | Purpose                                                                                                                                     | Must be green? |
| --------------- | ------------------------------------------------------------------------------------------------------------------------------------------- | -------------- |
| `meta`          | Enterprise files + toolchain pins present                                                                                                   | Yes            |
| `typescript`    | `pnpm install --frozen-lockfile` → `lint` → `typecheck` → `test` → `build`                                                                  | Yes            |
| `rust-core`     | `cargo fmt --check` → `clippy -D warnings` → `cargo test --workspace` (in `crates/`)                                                        | Yes            |
| `desktop-check` | Tauri shell compiles on Linux (apt WebKit2GTK + `fmt` + `clippy -D warnings` + `cargo check`; **no bundle**)                                | Yes            |
| `audit`         | `pnpm audit --audit-level high` + `pnpm audit:swarm`; uploads `docs/reports/` + `.talos/baselines.json`; prints baseline summary to the log | Yes            |
| `media-check`   | `docs/media/*` exist, README `docs/media/*` refs resolve, GIFs are `GIF89a`, `node scripts/media/generate-brand.mjs` leaves no diff         | Yes            |

Caching: `actions/setup-node` with `cache: pnpm`; `Swatinem/rust-cache@v2` for `crates` and `src-tauri` targets.

## Required checks

All six jobs are required. No `continue-on-error` jobs exist — the pipeline is intentionally honest: the full Tauri bundle (`pnpm build:desktop`) is **excluded** with a documented reason (platform bundlers, signing, display) rather than added as a flaky job.

## Reading green/red

- Green: all jobs pass; `audit` artifacts `talos-audit-reports` (contains `audit-latest.{md,json}`) and `talos-audit-baselines` (contains `.talos/baselines.json`) are uploaded even on failure (`if: always()`).
- Red `typescript`: open the failing step (`lint` = prettier/eslint, `typecheck` = `tsc`, `test` = vitest/node, `build` = vite/tsup). Reproduce locally with the same `pnpm` command.
- Red `rust-core`/`desktop-check`: usually `fmt` drift (run `cargo fmt`) or a new clippy warning (fix, never allow). Reproduce in `crates/` or `apps/desktop/src-tauri/`.
- Red `audit`: `pnpm audit --audit-level high` flags a high/critical advisory, the swarm found a `critical`, or the Baseline summary step reports a critical baseline regression. Open `docs/reports/audit-latest.md`, fix the `critical` rows (or update `.talos/baselines.json` gates with a documented reason), re-run `pnpm audit:swarm` locally.
- Red `media-check`: missing asset, broken README `docs/media/` link, invalid GIF header, or non-deterministic regeneration. Run `pnpm media:generate` locally and commit the result.

## Rerunning

Actions tab → failed run → **Re-run failed jobs** (preferred) or **Re-run all jobs**. For PRs, pushing a fix commit re-triggers automatically (concurrency cancels superseded runs per ref).

> Private repo note: the CI badge (`https://github.com/shubhanand021-EXA/talos/actions/workflows/ci.yml/badge.svg`) requires collaborator authentication to render.
