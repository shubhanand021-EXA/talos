# Talos Roadmap

> Tracker for `shubhanand021-EXA/talos` (`main`). See [CHANGELOG.md](../CHANGELOG.md) for versioned history and [docs/CI.md](CI.md) for gate behavior.

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

## Batch 2 — priorities (delivered as v0.2.0)

1. **Deeper agent runtime** — ✅ real task queue, agent lifecycle, cancellation/timeouts
2. **Real tool integrations** — ✅ filesystem + shell tools behind policy gates (+ Tool-RAG routing)
3. **Browser-use foundation** — ✅ scoped mock capability, dry-run-first, opt-in live Chromium
4. **Computer-use safety layer** — ✅ safety envelope, human-review gates, e-stop, mock-only
5. **Context budgeting / TypeCompact** — ✅ 7-layer budgets, deterministic compaction, proof hashes
6. **Expanded audit metrics** — ✅ lifecycle/cancellation suites, bundle budgets over time, baselines with WARN/FAIL
7. **Richer Void Neon interactions** — ✅ selection → inspector, live event pulses, kind filters, focus, replay

Delivered: `batch2-integration-demo` (`pnpm demo:batch2`), v0.2.0 tag, `docs/evidence/batch-2-final-report.md`.

Non-goals for Batch 2 (held): cloud accounts, external LLM keys in-repo, mobile targets.

## Batch 3 — Provider & orchestration track (delivered as v0.3.0 "The Awakening")

1. **Real provider/model adapter layer** — ✅ BYOK gateway (OpenAI/Anthropic/Ollama), keychain secrets, policy + budget gates
2. **Local model option** — ✅ Ollama behind the same adapter seam (no key, cheapest-adequate routing prefers it)
3. **Deeper live browser mode** — ✅ Playwright worker behind `TALOS_BROWSER_LIVE=1`, mock by default
4. **Real computer-use adapters** — ✅ `real-os-control` feature (accessibility APIs + vision loop), never CI/default
5. **Multi-agent orchestration strategies** — ✅ Supervisor decompose/validate/spawn/merge + task tree
6. **Memory/RAG expansion** — ✅ local embeddings, vector store, hybrid recall, skill proposals
7. **Production hardening** — ✅ delivered as Batch 4 (v0.4.0): encrypted at rest, crash recovery with resume, signed updater, OS sandbox, DAG planning, fidelity model, docs truthfulness

Delivered: `proof-of-awakening` demo (`pnpm demo:awakening`), v0.3.0 tag, `docs/evidence/batch-3-final-report.md`.

## Batch 4 — Hardening track (delivered as v0.4.0 "The Hardening")

1. **OS-level sandboxing** — ✅ bubblewrap/Seatbelt/Job Objects, fail-closed, `SandboxReceipt` in every exec receipt (§2b-iii, ADR 0010)
2. **LLM-driven DAG planning** — ✅ `PlanningPromptTemplate`, deterministic validation, level-ordered execution (ARCHITECTURE §2i)
3. **Execution fidelity** — ✅ typed mock/live/verified, persisted per task (§2j)
4. **Encrypted at rest** — ✅ ChaCha20-Poly1305 envelope, keychain-held 256-bit key, encrypted backups, plaintext migration, loud fallback (§2h)
5. **Crash recovery with resume** — ✅ dirty-shutdown flag, ledgered checkpoints every 5 tool calls, resume-or-fail-closed (§2h, migration 0010)
6. **Signed updater foundation** — ✅ ed25519 keypair flow, pinned pubkey, offline `scripts/sign-release.mjs`, `check_for_updates`/`install_update` IPC, Settings → Updates (§2k)
7. **Documentation truthfulness** — ✅ SECURITY.md rewritten, ARCHITECTURE/README/ROADMAP/PROGRESS/EVIDENCE swept, audit-swarm freshness checks
8. **MCP interoperability** — MCP client/server, instruction parsers, plugin trust, 80 IPC commands (ARCHITECTURE §2l, ADR 0011)
9. **GEPA skill learning loop** — deterministic extraction from verified tasks, watermarked evaluation every 15 receipts, usage-tracked evolution with versioning, published-skill retrieval + Tool-RAG boost, Skill Manager UI, 89 IPC commands (ARCHITECTURE §2e-i, ADR 0012)

Delivered: v0.4.0 version coherence (workspace, desktop, UI, contracts), `docs/evidence/batch-4-final-report.md` (to be written at release tagging).

- [x] **Batch 4 — Hardening (v0.4.0)**: OS sandbox, DAG planning, fidelity model, encrypted-at-rest evidence DB, crash recovery with checkpoint resume, signed updater foundation, truthful docs + swarm freshness checks.

## Batch 5+ — Autonomy & distribution (planned)

- [ ] LLM-driven decomposition, LLM-assisted extraction, HEARTBEAT.md execution, team/cloud foundation, staged-rollout release channel, advanced browser/computer-use, marketplace, mobile companion.

## Batch 2 — storefront checklist (historical record, Talos Batch 2 Prompt 01)

- [x] Public repo `shubhanand021-EXA/talos` created, `master` → `main`, full history + `v0.1.0-green-baseline` pushed (public since HF-03, see ADR 0005)
- [x] Metadata: description, 10 topics, public, `main` default, Issues on, Projects/Wiki off
- [x] Enterprise README (badges, hero GIF, feature grid, quickstart, architecture, Target Capability Matrix, roadmap, security, contributing, license, acknowledgement)
- [x] License posture confirmed MIT (existing defines project; no NOTICE); consistency across `Cargo.toml` + README
- [x] Community health: CONTRIBUTING / SECURITY / CODE_OF_CONDUCT / SUPPORT / CHANGELOG (Keep a Changelog) / CODEOWNERS / FUNDING / issue forms / PR template
- [x] Brand/media: `docs/media/{talos-logo,talos-banner,hero,evidence,architecture}` + `scripts/media/generate-brand.mjs` via `pnpm media:generate` (dependency-free, deterministic); Tauri icons kept as placeholders with documented next step
- [x] CI hardened to 6 jobs (`meta`, `typescript`, `rust-core`, `desktop-check`, `audit`, `media-check`) + caching + `workflows/README.md` + `docs/CI.md`; must pass green
- [x] Docs refreshed: ARCHITECTURE / PROGRESS / EVIDENCE / ROADMAP / CI / media kit
