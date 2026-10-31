# Changelog

All notable changes to the Talos green baseline. Dates are UTC. Versions follow semver from `0.1.0`.

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
