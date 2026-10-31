# Talos Roadmap

## Batch 1 — Green Baseline (complete, v0.1.0)

Goal was a green, auditable baseline with no product features. Delivered:

- [x] **Monorepo foundation** — pnpm workspaces, strict TS base, just runner, hygiene, git
- [x] **Shared contracts** — `@talos/contracts`: Zod schemas + inferred types for identity, agent, task, policy, evidence, compensation, system, IPC
- [x] **Rust backend** — `talos_common`, `policy_broker` (context-gated default-deny engine + selftest binary), `evidence_store` (SQLite + hash-chained log), `agent_runtime` (deterministic echo loop)
- [x] **Evidence ledger** — hash-chained `event_log`, claim lifecycle (pending/verified/rejected/conflicting), audit trail, compensation plans with guarded transitions; tamper detection tested
- [x] **React UI** — Vite + Tailwind, desktop chrome (TopBar/LeftRail/viewport/inspector/status bar), design system, 5 routes, deterministic mocks
- [x] **Void Neon 3D** — R3F viewport, emissive core, status-colored agents, link flows, quality tiers, reduced-motion + fallback
- [x] **Tauri shell** — `com.talos.desktop`, 5 typed IPC commands, least-privilege capabilities, strict CSP, heartbeat
- [x] **Local control plane** — UI → IPC → policy → evidence → events → UI, with Run Diagnostic UX
- [x] **Audit swarm** — 9 agents,_WARN-tolerant thresholds, reports in `docs/reports/`
- [x] **CI** — typecheck, lint, fmt, clippy, cargo tests, desktop check, node tests, build, dep audit, swarm, artifact upload

## Batch 2 — priorities (proposed)

1. **Deeper agent runtime** — real task queue, agent lifecycle beyond the echo loop, cancellation/timeouts
2. **Real tool integrations** — filesystem + shell tools behind policy gates (read first, writes with evidence)
3. **Browser-use foundation** — scoped browser capability, navigation observation, action allow-list
4. **Computer-use safety layer** — UI-action capability with human-review gates, input redaction
5. **Context budgeting / TypeCompact groundwork** — per-turn budgets, schema-first compaction in contracts
6. **Expanded audit metrics** — timing budgets, bundle tracking over time, flaky-test detection
7. **Richer Void Neon interactions** — node selection → inspector deep-links, edge animation on live events, scene filtering

Non-goals for Batch 2: cloud accounts, external LLM keys in-repo, mobile targets.
