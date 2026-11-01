<div align="center">

![Talos banner](docs/media/talos-banner.svg)

# Talos

**The open-source all-in-one AI agent ecosystem: coding, browser-use, computer-use, orchestration, evidence, and audit in one AAA-grade desktop command center.**

[![CI](https://github.com/shubhanand021-EXA/talos/actions/workflows/ci.yml/badge.svg)](https://github.com/shubhanand021-EXA/talos/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)
[![Release](https://img.shields.io/github/v/tag/shubhanand021-EXA/talos?label=release)](https://github.com/shubhanand021-EXA/talos/releases)
[![Stars](https://img.shields.io/github/stars/shubhanand021-EXA/talos?style=social)](https://github.com/shubhanand021-EXA/talos/stargazers)
[![Forks](https://img.shields.io/github/forks/shubhanand021-EXA/talos?style=social)](https://github.com/shubhanand021-EXA/talos/network/members)
[![Rust](https://img.shields.io/badge/rust-1.99.1-orange.svg?logo=rust)](crates/Cargo.toml)
[![Node](https://img.shields.io/badge/node-26_LTS-brightgreen.svg?logo=node.js)](.nvmrc)
[![pnpm](https://img.shields.io/badge/pnpm-10.34.7-blue.svg?logo=pnpm)](package.json)

> **Talos is public and open-source (MIT).** Badges above are live; CI status is authoritative in the [Actions tab](https://github.com/shubhanand021-EXA/talos/actions).

![Talos Void Neon command center](docs/media/hero.gif)

_Void-black command center — neon pulse, agent network motion, policy-gated control loop._

[Quickstart](#quickstart) · [Architecture](#architecture) · [Capability Matrix](#target-capability-matrix) · [Roadmap](#roadmap) · [Security](SECURITY.md) · [Contributing](CONTRIBUTING.md) · [Changelog](CHANGELOG.md)

</div>

---

## Why Talos

Fragmented agent tooling forces you to stitch together coding assistants, browser bots, computer-use hacks, orchestrators, and audit spreadsheets — with no shared policy, no shared evidence, and no shared desktop.

Talos is one monorepo, one contract layer, one policy broker, one evidence ledger, and one desktop shell:

- 🧩 **All-in-one agent ecosystem** — coding, browser-use foundation, computer-use safety layer, context budgeting + TypeCompact, local-first Tool-RAG routing, orchestration, evidence, and audit in a single workspace.
- 🛡️ **Policy-gated actions** — default-deny engine (`crates/policy_broker`); writes need evidence, irreversible actions need compensation plans.
- 🔗 **Evidence ledger** — hash-chained SQLite log (`crates/evidence_store`); claims move `pending → verified / rejected / conflicting` only through guarded transitions.
- 🧯 **Compensation plans** — `proposed → approved → executed / failed`, with `executed → rolled_back`; tamper detection tested.
- 🌐 **Browser-use foundation** — 9 policy-gated actions (`browser.navigate/click/type/scroll/select/extract/screenshot/snapshot/session`) with isolated per-task profiles, DOM-first safety (selector resolution + visibility checks), dry-run-first mutations, and ledgered receipts (`packages/browser-use` + `crates/tool_runtime`, Browser page at `/browser`). Mock by default; live Chromium is an explicit local opt-in (`TALOS_BROWSER_LIVE=1`, headless, 30s worker timeout, per-call spawned worker).
- 🖥️ **Computer-use safety layer** — 9 policy-gated actions (`computer.move_mouse/click/double_click/drag/type_text/hotkey/scroll/capture_screen/locate_element`) on a virtual screen with a mandatory safety envelope: global enable flag off by default, dry-run default, 30/min rate limit, viewport bounds, confidence floor, dangerous-zone blocking (settings, password managers), warnings for terminals and payment flows, and an emergency stop that cancels tasks and blocks everything until explicitly reset. Mock-only; real OS control is not implemented (`crates/computer_use`, Computer page at `/computer`).
- 🗜️ **Context budgeting + TypeCompact** — seven layered budgets (safety/system/task/session/evidence/tool/scratchpad, 8_000 tokens) with a deterministic local estimator (no external calls). TypeCompact preserves IDs, safety constraints, policy decisions, evidence/compensation refs, and schema type names; drops only reference-free segments on discardable layers; every result carries a `sha256:` proof hash and is invariant-gated before the receipt is ledgered (`crates/context_budget`, Context page at `/context`).
- 🔎 **Tool-RAG routing (local-first)** — SQLite-backed index over manifests, receipts, evidence records, task summaries, and compensation metadata (normalized tables, deterministic tokens, no FTS5, no network). Baseline lexical ranking blends keyword + capability match with receipt-history boosts and failure penalties, filtered by risk ceiling; embeddings arrive later behind a disabled-by-default provider seam (`crates/tool_rag`, panel on the Tools page).
- 🌌 **Void Neon 3D command center** — dark void, neon graph, live spatial topology: core + agents + tasks + tools + browser/computer-use + policy gate + evidence ledger + audit sentinel (`apps/ui` + R3F). Click to inspect, filter by family, focus a selection, replay recent events, watch policy/evidence pulses and the red emergency-stop overlay.
- 🧰 **Tool framework** — registered, schema-validated, policy-gated, evidence-backed tools (`crates/tool_runtime`): `fs.read/list/search`, `fs.write` (backup + compensation), `process.exec` (disabled by default, allowlisted). Dry-run first, receipts always ledgered. Manage in Tools page (`/tools`).
- 🕵️ **9-agent audit swarm** — security, privacy, reliability, performance, architecture, evidence, dependencies, UI/UX, product (`tools/audit`). Verifies safety defaults (tool/browser/computer execution gates), performance baselines (`.talos/baselines.json`: bundle sizes, gzip budgets, cargo subset + swarm durations), and deterministic behavior — WARN on regression, FAIL on critical.
- 🧠 **BYOK model gateway** — OpenAI, Anthropic, and local Ollama behind one policy-gated gateway (`crates/llm_gateway`): keys stay in the OS keychain (`crates/talos_secrets`), every call needs task/agent linkage, per-task spend caps deny before any traffic, and the ledger keeps usage + hashes (never prompts). Manage keys in Settings → Model providers. Dashboard on the Audit page.

> **Talos v0.2.0 — Batch 2 complete**: durable runtime, policy-gated tools, browser/computer-use safety layers, context budgeting + TypeCompact, local-first Tool-RAG, live Void Neon topology, and a hardened audit swarm — all local-first, deterministic, and evidence-backed. No real OS control, no external embeddings anywhere in the tree. See [docs/evidence/batch-2-final-report.md](docs/evidence/batch-2-final-report.md).

---

## Quickstart

Requires: `git >= 2.40`, Node.js LTS (see [.nvmrc](.nvmrc), currently v26), `pnpm >= 10` (pinned via `packageManager`), Rust stable + `cargo`, [just](https://github.com/casey/just) `>= 1.30`.

```sh
git clone https://github.com/shubhanand021-EXA/talos.git
cd talos
pnpm install        # or: just setup
pnpm typecheck      # strict tsc --noEmit
pnpm lint           # prettier + workspace lints
pnpm test           # root smoke + workspace suites
pnpm build          # workspace builds
pnpm audit:swarm    # 9-agent audit swarm (safety defaults + perf baselines)
pnpm media:generate # regenerate docs/media brand assets
```

Run it:

```sh
pnpm dev:ui       # browser-only UI (mock snapshot, disconnected banner)
pnpm dev:desktop  # Tauri window + live local loop (needs display + WebView2)
```

Prove Batch 2 end-to-end (headless: Rust flow + topology tests + audit swarm):

```sh
pnpm demo:batch2  # or: just demo-batch2
```

Validate Rust:

```sh
cd crates && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Full command matrix and headless caveats: [docs/RUNBOOK.md](docs/RUNBOOK.md) · CI behavior: [docs/CI.md](docs/CI.md).

---

## Architecture

![Talos architecture](docs/media/architecture.svg)

**Pentagon: Foundation · Brain · Soul · Face · Shield.**

```text
talos/
  apps/
    desktop/      # Tauri 2 shell (com.talos.desktop), 5 typed IPC commands
    ui/           # React 19 + Vite + Tailwind + Zustand + Void Neon viewport
  crates/
    talos_common/   # typed IDs, error model, time/serde/tracing helpers
    policy_broker/  # default-deny engine + context hooks + --selftest
    evidence_store/ # SQLite + hash-chained event_log, claims, compensation
    agent_runtime/  # LocalOrchestrator: diagnostic loop + durable task queue (lifecycle, worker, supervisor)
  packages/
    contracts/    # @talos/contracts — Zod schemas, single source of truth
  tools/
    audit/        # @talos/audit-swarm — 9 agents, Markdown + JSON reports
  docs/
    ARCHITECTURE.md PROGRESS.md EVIDENCE.md ROADMAP.md CI.md RUNBOOK.md
    media/        # logo, banner, hero.gif, evidence.gif, architecture.svg
    reports/      # audit-latest.{md,json} + dated copies
  scripts/
    media/        # generate-brand.mjs — deterministic, dependency-free
  .github/
    workflows/ci.yml + README + CODEOWNERS + ISSUE_TEMPLATE + PR template
```

Local control loop: **UI → IPC → Policy → Evidence → Events → UI**.

- `run_diagnostic` executes a 3-action echo loop (read Allow, write → evidence → Allow, irreversible → plan → Allow).
- `submit_task` enqueues durable work (SQLite: queue, attempts, transitions, cancellations); the worker executes through the policy broker with priority order, per-attempt timeouts, retryable backoff, and cooperative cancellation. `cancel_task` / `retry_task` / `list_tasks` / `get_task_state` are live in the Agents page and Dashboard queue table.
- Every side effect passes `PolicyContext` gates — no bypass paths.
- Evidence page verifies the chain live (`verify_evidence_ledger`); broken fixtures render `CHAIN BROKEN` with `first bad seq`.

![Evidence ledger verification](docs/media/evidence.gif)

_Ledger verification — hash-chain validation, claim lifecycle, policy allow/deny states._

Deep dive: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) · [docs/EVIDENCE.md](docs/EVIDENCE.md) · [docs/PROGRESS.md](docs/PROGRESS.md).

---

## Target Capability Matrix

> **Target Capability Matrix** — design intent for the completed Talos track, not a claim about third-party products. Fragmented columns describe typical single-purpose tool categories; Talos aims to unify them under one policy + evidence + audit roof.

| Capability                         | Typical coding agents | Typical browser agents | Typical computer-use agents | Typical orchestration tools | Typical audit systems | **Talos target**                                                         |
| ---------------------------------- | --------------------- | ---------------------- | --------------------------- | --------------------------- | --------------------- | ------------------------------------------------------------------------ |
| Coding + repo tools                | ✅                    | ❌                     | ❌                          | ⚠️                          | ❌                    | ✅ policy-gated fs/shell (read first, writes with evidence)              |
| Browser-use (scoped, allow-listed) | ❌                    | ✅                     | ⚠️                          | ❌                          | ❌                    | ✅ navigation observation + action allow-list                            |
| Computer-use (human-review gates)  | ❌                    | ❌                     | ✅                          | ❌                          | ❌                    | ✅ UI-action capability + redaction + approval                           |
| Orchestration (multi-agent, queue) | ⚠️                    | ❌                     | ❌                          | ✅                          | ❌                    | ✅ task queue, lifecycle, cancellation/timeouts                          |
| Policy broker (default-deny)       | ❌                    | ❌                     | ❌                          | ❌                          | ❌                    | ✅ central authority, context hooks, no bypass                           |
| Evidence ledger (claims vs proof)  | ❌                    | ❌                     | ❌                          | ❌                          | ⚠️                    | ✅ hash-chained log, guarded transitions, tamper tests                   |
| Compensation / rollback plans      | ❌                    | ❌                     | ❌                          | ⚠️                          | ❌                    | ✅ proposed → approved → executed → rolled_back                          |
| Desktop command center (Void Neon) | ❌                    | ❌                     | ❌                          | ❌                          | ❌                    | ✅ Tauri 2 + React + 3D topology, offline-first                          |
| 9-agent audit swarm                | ❌                    | ❌                     | ❌                          | ❌                          | ⚠️                    | ✅ 9 agents, safety defaults, perf baselines, Markdown + JSON, CI-gated  |
| Context budgeting + TypeCompact    | ❌                    | ❌                     | ❌                          | ⚠️                          | ❌                    | ✅ layered budgets, invariant-checked compaction, proof hashes           |
| Local-first Tool-RAG routing       | ❌                    | ❌                     | ❌                          | ⚠️                          | ❌                    | ✅ lexical ranking, risk ceilings, receipt-history boosts, no embeddings |
| Live 3D operations topology        | ❌                    | ❌                     | ❌                          | ❌                          | ❌                    | ✅ 9 node kinds, filters, focus, replay, e-stop overlay, offline-first   |

Legend: ✅ native target · ⚠️ partial in typical tools · ❌ generally absent. No third-party product is named or evaluated here.

---

## Model providers (BYOK)

Talos calls real models through `crates/llm_gateway` — BYOK by default, no keys in repo, config, or telemetry:

- **Providers**: OpenAI (`gpt-4o`, `gpt-4o-mini`, `o3-mini`), Anthropic (`claude-sonnet-4-20250514`, `claude-3-5-haiku`), local Ollama (dynamic `/api/tags` discovery, no key).
- **Keys**: stored in the OS keychain (`com.talos.desktop`, `crates/talos_secrets`); the UI password field never displays a value back, and failures report `secret_unavailable` without details.
- **Policy gating**: every call is a `NetworkRequest` action with task/agent linkage, evaluated with a recorded request receipt as evidence; over-budget tasks deny before any traffic.
- **Evidence**: `usage_records` (tokens, cost, latency) + `model_interactions` (SHA-256 of metadata only) — prompts and completions are never persisted.
- **Manage**: Settings → Model providers (status, Add/Test/Remove key, model catalog, session usage). Room for a future Talos-managed provider behind the same trait.

Details: [docs/adr/0006-llm-gateway-byok.md](docs/adr/0006-llm-gateway-byok.md).

---

## Roadmap

- [x] **Batch 1 — Green Baseline (v0.1.0)**: monorepo, contracts, policy broker, evidence ledger, React shell, Void Neon baseline, Tauri shell, local loop, audit swarm, CI.
- [x] **Batch 2 — Durable runtime**: SQLite-backed task queue (entries, attempts, transitions, cancellations), strict 10-state lifecycle, priority scheduling, timeouts, retry policy, cooperative cancellation, supervisor, seeded MockEchoAgent scenarios, 5 new IPC commands, live queue UI with cancel/retry + task-filtered feed.
- [x] **Batch 2 — Tool framework**: `crates/tool_runtime` (model, registry, adapters, 5 built-ins, policy-gated executor with evidence + compensation), 4 new IPC commands (`list/describe/dry-run/invoke`), Tools page (`/tools`) with catalog + dry-run form + receipt viewer, 15 runtime + 6 desktop tool tests.
- [x] **Batch 2 — Browser-use foundation**: 8 policy-gated browser tools + `packages/browser-use` (isolated sessions, mock adapter, opt-in live Chromium, worker CLI), Browser page (`/browser`) with action builder + dry-run + receipt/hash viewer, ADR `docs/adr/0001-browser-use-architecture.md`.
- [x] **Batch 2 — Computer-use safety layer**: `crates/computer_use` (safety envelope, mock virtual screen, disabled real-adapter stub), 9 policy-gated `computer.*` tools, UiAction policy rules (Low allow / Medium evidence-gated / High human review), emergency stop + cancel + reset IPC, Computer page (`/computer`) with safety panel, mock screen, receipt viewer, and status-bar indicator, ADR `docs/adr/0002-computer-use-safety-layer.md`.
- [x] **Batch 2 — Context budgeting + TypeCompact**: `crates/context_budget` (7-layer model, local char-fallback estimator, priority-ordered rebalancing, deterministic TypeCompact, structured invariant gate), contracts `context.ts`, 3 IPC commands (`contextBudgetStatus`, `contextCompact`, `contextDiagnosticSnapshot`) + `contextCompacted` event, Context page (`/context`) with budget bars, compacted preview, invariant checklist, receipt panel, protected items list, ADR `docs/adr/0003-context-budget-typecompact.md`.
- [x] **Batch 2 — Tool-RAG routing foundation**: `crates/tool_rag` (SQLite index over manifests/receipts/evidence/tasks/compensation, deterministic tokenizer, lexical provider + disabled embedding stub, baseline ranking), contracts `toolrag.ts`, IPC command `queryToolCandidates` (read-only), Tool-RAG panel on the Tools page (intent box, capability/risk/evidence/top-K filters, ranked candidates with score/reason/risk badge/approvals, manifest links), ADR `docs/adr/0004-tool-rag-foundation.md`.
- [x] **Batch 2 — Void Neon live topology**: interactive operational viewport — 9 node kinds (core/agents/tasks/tools/browser/computer-use/policy/evidence/audit) with state-mapped colors, task-assignment / invocation / policy / evidence edges, click-select + hover tooltips + focus mode, kind filters, inspector actions (cancel/retry/view receipt/view evidence), recent-activity glow + event pulses + replay slider, HUD counts + e-stop overlay, deterministic topology builder with caps and zero per-frame allocation, `hero.gif` regenerated with the new glyphs.
- [x] **Batch 2 — Audit swarm hardening**: expanded subchecks in all 9 agents (tool/browser/computer execution defaults, human-review gating, lifecycle + cancellation suites, profile isolation, receipt/proof-hash linkage, manifest license compatibility, critical-action reachability, e-stop visibility, state affordances, README feature coverage), `.talos/baselines.json` performance gates (bundle sizes, lazy-chunk gzip, cargo subset + swarm durations) with WARN/FAIL regression policy, live Audit dashboard (report summary, agent grid, baseline panel), CI uploads reports + baselines with a log summary.
- [x] **Batch 2 — Integration + v0.2.0 release**: deterministic `batch2-integration-demo` (runtime → Tool-RAG → tools → browser/computer mocks → TypeCompact → compensation → ledger → audit → topology), `pnpm demo:batch2` / `just demo-batch2`, enterprise storefront polish, version coherence at 0.2.0.
- [ ] **Batch 3+ — Providers & orchestration**: real model/provider adapter layer, local model option, deeper live browser mode, real computer-use adapters behind safety flags, multi-agent orchestration strategies, memory/RAG expansion, file-backed encrypted ledger, production hardening.

Details: [docs/ROADMAP.md](docs/ROADMAP.md) · history: [CHANGELOG.md](CHANGELOG.md).

---

## Security

Local-first threat model. Policy broker is the only authority; Tauri runs least-privilege (zero fs/shell/HTTP/dialog permissions); ledger is append-only with tamper tests; audit swarm scans for secrets on every run.

- Do **not** report vulnerabilities in public issues.
- See [SECURITY.md](SECURITY.md): `security@talos.dev` placeholder + GitHub private vulnerability reporting via `Security → Report a vulnerability`.
- Operational notes: [docs/RUNBOOK.md](docs/RUNBOOK.md).

---

## Contributing

Single source of truth (`@talos/contracts`), policy-central side effects, evidence-first claims, audit-green pushes.

- Dev setup, branch model (`main` + short-lived `feat|fix|chore|docs/` branches), Conventional Commits, testing + audit gates, PR checklist: [CONTRIBUTING.md](CONTRIBUTING.md).
- Issue forms: [`.github/ISSUE_TEMPLATE`](.github/ISSUE_TEMPLATE) · PR template: [`.github/PULL_REQUEST_TEMPLATE.md`](.github/PULL_REQUEST_TEMPLATE.md).
- Owners: [`.github/CODEOWNERS`](.github/CODEOWNERS).

```sh
pnpm install
pnpm typecheck && pnpm lint && pnpm test && pnpm build
pnpm audit && pnpm audit:swarm
```

---

## License

MIT — see [LICENSE](LICENSE). Workspace crates declare `license = "MIT"`; UI/contracts/desktop packages are `private: true` (not published).

---

## Acknowledgements

Led by **Shubh Anand**.

Original brand system generated offline via `scripts/media/generate-brand.mjs` — no external assets, no copyrighted inputs. Tauri icons in `apps/desktop/src-tauri/icons/` remain generated placeholders; see [docs/media/README.md](docs/media/README.md) for the replacement step.
