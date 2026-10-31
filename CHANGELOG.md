# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased] — Batch 2 storefront

### Added

- Private GitHub repository `shubhanand021-EXA/talos` with full Batch 1 history and `v0.1.0-green-baseline` tag.
- Enterprise storefront: premium README with badges, hero GIF, feature grid, quickstart, architecture, Target Capability Matrix, roadmap, security, contributing, license, acknowledgements.
- Brand/media system: `scripts/media/generate-brand.mjs` (`pnpm media:generate`), original `talos-logo.svg`, `talos-banner.svg`, `hero.gif`, `evidence.gif`, `architecture.svg` under `docs/media/` (deterministic, dependency-free, offline).
- GitHub community health: `CONTRIBUTING.md`, `SECURITY.md`, `CODE_OF_CONDUCT.md` (Contributor Covenant 2.1), `SUPPORT.md`, `.github/CODEOWNERS`, `.github/FUNDING.yml`, issue forms (`bug_report`, `feature_request`, `security_report`, `question`, `config`), PR template with audit/risk/rollback sections.
- Hardened CI (`.github/workflows/ci.yml`): push/PR/dispatch triggers, pnpm + cargo caching, `setup/meta`, `typescript`, `rust-core`, `desktop-check`, `audit`, `media-check` jobs, artifact upload, honest isolation of platform-dependent steps; documented in `.github/workflows/README.md` and `docs/CI.md`.
- Docs refresh: `docs/ARCHITECTURE.md`, `docs/PROGRESS.md`, `docs/EVIDENCE.md`, `docs/ROADMAP.md`, new `docs/CI.md`, `docs/media/README.md`.

### Changed

- Default branch is `main` (renamed from `master`); remote `origin` tracks `https://github.com/shubhanand021-EXA/talos.git`.
- Repository metadata: private visibility, description, topics (`ai`, `agents`, `ai-agents`, `desktop-app`, `tauri`, `rust`, `react`, `typescript`, `orchestration`, `browser-automation`), Issues enabled, Projects/Wiki disabled.
- License posture confirmed as MIT (existing `LICENSE` retained; see notes below).

### Notes

- License decision: Batch 1 shipped MIT (`LICENSE`, `crates/Cargo.toml license = "MIT"`, `apps/desktop/src-tauri/Cargo.toml license = "MIT"`, README MIT badge). The Batch 2 brief suggested Apache-2.0 unless an existing license defines the project differently — since MIT already defines it, MIT is retained for consistency and no `NOTICE` file is required.

## [0.1.0] — 2026-10-31 — Green Baseline

First reproducible baseline: monorepo, shared contracts, Rust policy broker + evidence ledger, React shell + Void Neon viewport, Tauri desktop shell, local echo-agent loop, 9-agent audit swarm, CI.

### Added

- Monorepo foundation: pnpm workspaces, strict TypeScript base, just task runner, hygiene, CI (`chore: bootstrap talos monorepo foundation`)
- `@talos/contracts` 0.1.0: Zod schemas + inferred types for identity, agent, task, policy, evidence, compensation, system, IPC (`feat(contracts)`)
- Rust workspace: `talos_common`, `policy_broker` (context-gated default-deny engine + selftest binary), `evidence_store` (SQLite + hash-chained log) (`feat(rust)`)
- Agent runtime: `LocalOrchestrator` + `MockEchoAgent`, 3-action diagnostic flow with claim + compensation plan (`feat(runtime)`, `feat(evidence)`)
- Evidence ledger: hash-chained `event_log`, claim lifecycle, audit trail, compensation plans with guarded transitions, tamper-detection tests (`feat(evidence)`)
- React app shell: desktop chrome, design system, 5 routes, Zustand stores, deterministic mocks (`feat(ui)`)
- Void Neon 3D viewport: Three.js + R3F, quality tiers, reduced-motion + fallback (`feat(ui)`)
- Tauri 2 shell (`com.talos.desktop`): 5 typed IPC commands, least-privilege capabilities, strict CSP, heartbeat (`feat(desktop)`)
- Typed bridge (`apps/ui/src/bridge`) + `useTalosSync`: validated invoke, event subscriptions, live stores (`feat(runtime)`)
- Audit swarm (`@talos/audit-swarm`): 9 agents, config-driven thresholds, Markdown + JSON reports (`feat(audit)`)
- Docs: ARCHITECTURE, PROGRESS, EVIDENCE, ROADMAP, RUNBOOK, SECURITY, CONTRIBUTING

### Verification

- `just setup|lint|format|typecheck|test|build|audit|doctor|dev-ui|dev-desktop` wired; see `docs/evidence/batch-1-final-report.md`
- Audit: WARN (duplicates only), 0 criticals; tag `v0.1.0-green-baseline`

[Unreleased]: https://github.com/shubhanand021-EXA/talos/compare/v0.1.0-green-baseline...HEAD
[0.1.0]: https://github.com/shubhanand021-EXA/talos/releases/tag/v0.1.0-green-baseline
