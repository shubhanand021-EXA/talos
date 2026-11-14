# Talos CI

CI workflow: [`.github/workflows/ci.yml`](../.github/workflows/ci.yml) (see also [`.github/workflows/README.md`](../.github/workflows/README.md)).

## What runs

Triggers: `push` to `main` (docs-only pushes filtered via `paths-ignore`), `pull_request` to `main` (always full matrix, so docs-only PRs stay mergeable under the ruleset), `workflow_dispatch`, plus a nightly `schedule` run. The nightly schedule still covers the tree when pushes are docs-only.

| Job               | Purpose                                                                                                                                                                                                      | Must be green? |
| ----------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | -------------- |
| `meta`            | Enterprise files + toolchain pins present                                                                                                                                                                    | Yes            |
| `typescript`      | `pnpm install --frozen-lockfile` → build workspace types (`contracts`, `browser-use` — lint is type-aware) → `lint` → `typecheck` → `test` → `build`                                                         | Yes            |
| `rust-core`       | `cargo fmt --check` → `clippy -D warnings` → `cargo test --workspace` (in `crates/`)                                                                                                                         | Yes            |
| `desktop-check`   | Tauri shell compiles on Linux (apt WebKit2GTK + `fmt` + `clippy -D warnings` + `cargo check`; **no bundle**)                                                                                                 | Yes            |
| `desktop-test`    | Tauri shell unit tests EXECUTE on Linux (same WebKit2GTK prerequisites + `xvfb-run -a cargo test` headless)                                                                                                  | Yes            |
| `audit`           | `pnpm audit --audit-level high` + `pnpm build` (dist required for perf baselines) + `pnpm audit:swarm`; uploads `docs/reports/` + `.talos/baselines.json`; prints baseline summary to the log                | Yes            |
| `media-check`     | `docs/media/*` exist, README `docs/media/*` refs resolve, GIFs are `GIF89a`, `node scripts/media/generate-brand.mjs` leaves no diff                                                                          | Yes            |
| `stress-shutdown` | 100-cycle pool-shutdown stress + historically flaky seal tests on ubuntu + windows (pinned Rust 1.99.1; macOS omitted — runners never dispatch, see below)                                                   | Yes            |
| `parity`          | `docs/parity/CAPABILITY_MATRIX.json` schema + evidence + deterministic SUMMARY.md + charter checks (`node scripts/parity/validate-matrix.mjs --check`); drift or invented competitor cells FAIL              | Yes            |
| `perf`            | Deterministic P3 harness (`node scripts/perf-harness.mjs --runs 20 --check`): seeded mock-data CPU proxies + real dist bytes vs `.talos/baselines.json` fail gates (exit 1 on breach); no display dependency | Yes            |
| `flake-watch`     | Rerun accounting reminder (non-blocking; never fails)                                                                                                                                                        | No             |

Caching: `actions/setup-node` with `cache: pnpm`; `Swatinem/rust-cache@v2` for `crates` and `src-tauri` targets. Every job carries a `timeout-minutes` cap; uploaded artifacts carry `retention-days: 14`.

## Runner pins (Prompt 017)

All Linux jobs float `ubuntu-latest` — deliberately, not by omission.
Attempt history (this prompt, CI run `43591652114` att.1–2): pinning
`ubuntu-24.04` broke every bubblewrap-backed gate with
`bwrap: loopback: Failed RTM_NEWADDR: Operation not permitted`
(`rust-core` specialists test + `hardening` sandbox-live demo, identical
signature twice), while `ubuntu-latest` — which resolves to
**ubuntu-26.04** now that the Oct-2026 migration is complete — is green
across the matrix. The 24.04 image carries the AppArmor
unprivileged-userns restriction that blocks bwrap's loopback setup;
26.04 does not. So the 26-readiness answer is: ride `latest` (= 26.04,
verified green), do NOT pin 24.04. Revisit only if `latest` moves again.
Actions run current Node-24-compatible majors
(checkout/setup-node/upload-artifact v7, pnpm/action-setup v6);
`dtolnay/rust-toolchain@stable` and `Swatinem/rust-cache@v2` are unchanged
(already current, toolchain itself stays pinned).

## Required checks

The `main protection` ruleset enforces six status checks: `setup / meta`,
`typescript (lint / typecheck / test / build)`, `rust-core (fmt / clippy / test)`,
`desktop-check (tauri shell compiles)`, `audit (dep audit + swarm)`, and
`media-check (assets + README refs)`. Repo convention additionally requires
`awakening`, `hardening`, the `desktop-test` (shell unit tests execute),
the `stress-shutdown`, the `parity` (matrix +
charter gate), and the `perf` (P3 harness budgets) jobs green on every PR —
they run on all OSes (stress) / ubuntu and a merge with any of them red
is treated as a BLOCKED prompt. (The GitHub ruleset names the six core
checks; the convention-required jobs are enforced by review, and the audit
swarm's parity agent independently fails the `audit` job on violations.) The manual `browser live smoke` and the
reminder-only `flake-watch` are exempt. No `continue-on-error` jobs exist — the pipeline is intentionally honest: the full Tauri bundle (`pnpm build:desktop`) is **excluded** with a documented reason (platform bundlers, signing, display) rather than added as a flaky job.

## Flake policy (no silent reruns)

1. At most ONE rerun per CI run (`Re-run failed jobs`, never `Re-run all jobs`
   unless the whole matrix is suspect).
2. A rerun that goes green MUST produce a tracked flake issue carrying the run
   URL, the attempt number, and the failing test id — and a row in the
   quarantine register (`docs/evidence/FLAKES.md`: test id, owner, deadline
   ≤ 7 days, status). The `flake-watch` job prints this reminder on every
   attempt > 1; it never fails.
3. The fix for a quarantined flake lands in its own prompt; the register row
   flips to `fixed by <PR>` on merge. Deleting a flaky test or weakening its
   assertion to force green is forbidden — quarantine it with an owner and a
   deadline instead.

## Reading green/red

- Green: all jobs pass; `audit` artifacts `talos-audit-reports` (contains `audit-latest.{md,json}`) and `talos-audit-baselines` (contains `.talos/baselines.json`) are uploaded even on failure (`if: always()`).
- Red `typescript`: open the failing step (`lint` = prettier/eslint, `typecheck` = `tsc`, `test` = vitest/node, `build` = vite/tsup). Reproduce locally with the same `pnpm` command.
- Red `rust-core`/`desktop-check`: usually `fmt` drift (run `cargo fmt`) or a new clippy warning (fix, never allow). Reproduce in `crates/` or `apps/desktop/src-tauri/`.
- Red `audit`: `pnpm audit --audit-level high` flags a high/critical advisory, the swarm found a `critical`, or the Baseline summary step reports a critical baseline regression. Open `docs/reports/audit-latest.md`, fix the `critical` rows (or update `.talos/baselines.json` gates with a documented reason), re-run `pnpm audit:swarm` locally.
- Red `media-check`: missing asset, broken README `docs/media/` link, invalid GIF header, or non-deterministic regeneration. Run `pnpm media:generate` locally and commit the result.

## Rerunning

Actions tab → failed run → **Re-run failed jobs** (preferred) or **Re-run all jobs**. For PRs, pushing a fix commit re-triggers automatically (concurrency cancels superseded runs per ref).

> This repository is public (see [ADR 0005](adr/0005-public-repository-transition.md)): the CI badge above renders for everyone, and standard-runner minutes are free and unlimited. No billing-block language applies to this repo.
