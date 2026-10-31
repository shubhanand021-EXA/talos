# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased] — Batch 2 browser-use

### Added

- Browser-use foundation: 8 policy-gated tools (`browser.navigate/click/type/scroll/select/extract/screenshot/snapshot`) in `crates/tool_runtime` (registry manifests, strict validation, deterministic mock DOM/session, gated executor, evidence receipts with dom/screenshot/text hashes).
- Policy: `OpenBrowser`-Medium evidence gate (dry-run first, then allow); `highRisk` flags (form submit, purchase, settings, upload/download, destructive) escalate to human review.
- `packages/browser-use`: isolated per-task sessions, DOM-first safety (resolution reports, visibility/interactability checks), deterministic `MockBrowserAdapter`, opt-in `LiveBrowserAdapter` (Playwright, disabled without `TALOS_BROWSER_LIVE=1`), worker CLI (`talos-browser`), 9 tests, README with live-mode setup.
- Contracts `browser.ts`: `BrowserAction` (8), `BrowserTarget`, `BrowserActionRequest`, `SelectorReport`, `BrowserReceipt`; `ToolId` extended to 13 tools.
- UI Browser page (`/browser`): action builder, task linkage, dry-run/live buttons, policy panel, receipt viewer (dom/screenshot/text hashes), high-risk safety warnings; nav + command palette entries.
- ADR `docs/adr/0001-browser-use-architecture.md`: Rust authoritative, TS worker + opt-in live adapter, contracts-first, isolation and gating model.
- Tests: 24 `tool_runtime` (browser lifecycle, gating, receipts, isolation), 10 policy broker (new browser rule), 9 browser-use package, 3 Browser page UI, desktop registry count updated to 13.

## [Unreleased] — Batch 2 tools

### Added

- Tool integration framework (`crates/tool_runtime`): `ToolId` / `ToolManifest` (id, name, version, description, capabilities, risk profile, reversibility, evidence/compensation requirements, JSON input/output schemas) / `ToolInvocation` / `ToolReceipt` (status, policy verdict, evidence/compensation links) + `ToolRegistry` (register/list/get/validate, stable order, metadata for UI + Tool-RAG).
- Safe built-ins: `fs.read` (metadata, hash, UTF-8 text), `fs.list` (bounded depth/entries, no symlink escape), `fs.search` (bounded files/matches, skips binary), `fs.write` (policy approval + evidence receipt + `backup_restore` compensation before mutation), `process.exec` (disabled by default, dry-run first, allowlist, timeout, captured output, no shell).
- Execution adapters: `DryRun` (default for risky tools, no side effects) / confined `LocalSafe` / future `Sandbox` placeholder (honest unavailable).
- Policy integration: every invocation becomes an `ActionRequest` (risk from manifest); write/exec require evidence; denied/disabled produce structured `policy_denied` / `tool_disabled` errors; all receipts stored as `tool_receipt` evidence (verified).
- Desktop IPC: `list_tools`, `describe_tool`, `invoke_tool_dry_run`, `invoke_tool` (policy-gated, audited; 17 desktop tests green).
- UI Tools page (`/tools`): catalog table (risk, reversibility, enablement, capabilities), manifest details, dry-run form (task id + JSON params + templates), receipt viewer (status pill, policy decision, evidence/compensation links, output JSON), nav + command palette entries.
- Tests: 15 `tool_runtime` (manifest, validation, dry-run purity, fs safety, write gate + compensation, exec disabled, receipts, symlink), 6 desktop tool IPC, 3 Tools page UI; contracts `tool.test.ts` (ids, statuses, schemas).

## [Unreleased] — Batch 2 runtime

### Added

- Durable agent runtime: strict 10-state lifecycle (`queued → planning → executing → waiting_* → completed/failed/timed_out`, `canceling → canceled`, explicit retry `failed/timed_out/canceled → queued`), SQLite persistence (`0003_runtime`: `runtime_tasks/attempts/transitions/cancellations` + retry metadata), every transition journaled + `task.transition` ledgered.
- Scheduling: priority ordering (critical/high/normal/low), `RuntimeConfig` concurrency limit, per-attempt timeout, retry policy (max attempts, deterministic backoff, retryable vs fatal), cooperative `CancelToken` + DB cancellation (requested/acknowledged/completed).
- Supervisor (`supervise_once`): timeout enforcement, stuck detection, cancellation respect, policy-respecting retries; never bypasses the broker.
- MockEchoAgent scenarios (seeded, deterministic): quick/delayed success, retryable/non-retryable failure, cancellation-sensitive; `outcome_for_attempt` covered by tests.
- Runtime API: `submit_task`, `cancel_task`, `retry_task`, `get_task_state`, `list_tasks`, `list_queue` (typed, validated).
- Desktop IPC: `submit_task`, `cancel_task`, `retry_task`, `list_tasks`, `get_task_state` (existing 5 commands untouched; 12 desktop tests green).
- UI: live task queue table (Agents + Dashboard) with status badges, priority labels, attempt counts, Cancel/Retry buttons; task detail in RightInspector; event feed filters by selected task; `useTalosSync` handles `taskUpdated` via `getTaskState` upsert.
- Tests: lifecycle valid/invalid, priority ordering, retry requeue, fatal terminal, cancellation, timeout, persistence reload, ledger recording (Rust 16 runtime tests); queue store + table component (UI 45 tests).

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
