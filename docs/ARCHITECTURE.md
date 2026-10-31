# Talos Architecture — Pentagon Architecture

> Repo: `https://github.com/shubhanand021-EXA/talos` (private, default branch `main`). Batch 2 storefront: enterprise README, brand/media system (`docs/media/`), hardened 6-job CI ([docs/CI.md](CI.md)). Control-loop diagram: [docs/media/architecture.svg](media/architecture.svg).

Batch 1 status: **Foundation and Shield baseline only**. Brain, Soul, and Face are specified here but not implemented until later batches.

## 1. Foundation: Tauri 2 + Rust + React

- Desktop shell in `apps/desktop/`
- UI package in `apps/ui/`
- Rust workspace in `crates/`
- Shared TS contracts in `packages/contracts/`
- pnpm workspaces + strict TypeScript + `just` task runner
- Reproducible setup via `just setup` and `just doctor`

Batch 1 scope: monorepo skeleton, tooling, hygiene, CI, shared TS contracts,
Rust workspace with Policy Broker core + SQLite evidence store + agent runtime + hash-chained ledger and compensation plans,
Tauri desktop shell with typed IPC and live local loop (no LLMs, no network).
Remaining Batch 1 work: file-backed persistence, real (non-mock) agents, full audit swarm.

## Desktop shell (`apps/desktop/`)

- Tauri 2 app `com.talos.desktop`, window `Talos` (1440×900, min 1280×800)
- `frontendDist: ../ui/dist` (production), `devUrl: http://localhost:5173` + `beforeDevCommand: pnpm --filter ui dev`
- `src-tauri` is a standalone Cargo workspace with path deps into `crates/`
- `AppState`: `DefaultPolicyEngine` + SQLite `EvidenceDb` (migrations `0001-0003`) + `LocalOrchestrator` (Mutex-guarded), managed by Tauri
- Ten typed IPC commands (`get_app_info`, `get_system_status`, `request_action`,
  `run_diagnostic`, `verify_evidence_ledger`, `submit_task`, `cancel_task`,
  `retry_task`, `list_tasks`, `get_task_state`); structured `{ code, message }` errors
- `run_diagnostic` runs the echo-agent loop and emits one event per stage; `submit_task` enqueues durable work and executes it through the policy broker; 5s `heartbeat`
- UI bridge (`apps/ui/src/bridge/`): typed `invoke` + Zod validation, `useTalosSync` subscription hook, stores hold live agents/feed/evaluations
- Capability `default`: main window only, zero extra permissions; strict CSP
- Event names string-identical to `@talos/contracts` `IpcEventName`
- Root scripts: `pnpm dev:desktop`, `pnpm build:desktop` (`just dev-desktop` / `just build-desktop`)

## Rust workspace layout (`crates/`)

```text
crates/
  Cargo.toml          # workspace: talos_common, policy_broker, evidence_store, agent_runtime
  talos_common/       # TalosError/Result, typed IDs, time + serde + tracing helpers
  policy_broker/      # PolicyEngine trait + DefaultPolicyEngine + context hooks, talos-policy-broker --selftest
  evidence_store/     # SQLite store, migrations (0001 init, 0002 ledger, 0003 runtime queue), append-only repo
  agent_runtime/      # lifecycle state machine + durable queue + worker/supervisor + MockEchoAgent scenarios
```

- Workspace lints forbid `unsafe_code`; binaries commit `Cargo.lock`.
- Policy decisions mirror `@talos/contracts`: allow / deny / require_evidence /
  require_compensation / require_human_review, default deny.
- Quality gates: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`, `talos-policy-broker --selftest`.

## 2. Brain: Tool-RAG + context budgeting + TypeCompact

- Tool registry with retrieval (Tool-RAG)
- Explicit context budgets per turn
- TypeCompact: schema-first compaction for long histories

Status: **specified, not implemented**. Target: Batch 3+.

## 3. Soul: Evidence vs Claims ledger + Compensation Plans

- Every agent claim must link to runnable evidence (command, output, commit, artifact)
- `docs/EVIDENCE.md` is the human-readable ledger in Batch 1
- Machine ledger (Batch 1, local): hash-chained `event_log` + evidence chain,
  claims with pending/verified/rejected/conflicting lifecycle, audit trail
  (`action_requests`, `policy_evaluations`), compensation plans with
  proposed→approved→executed/failed and executed→rolled_back transitions
- Policy gates: write-like actions require recorded evidence; irreversible
  actions require a compensation plan — enforced via `PolicyContext` hooks,
  no bypass

Status: **ledger + compensation foundation live locally; durable runtime queue (0003) persists entries, attempts, transitions, cancellations with `task.transition` ledgering; file-backed DB path arrives later**.

## 4. Face: Void Neon 3D Spatial Topology UI

- AAA-grade desktop UX: dark void, neon graph, spatial topology of agents/tools/memory
- React + Vite + 3D (Batch 4+)

Status: **specified, not implemented**.

## 5. Shield: 9-Agent Audit Swarm

- 9 independent audit agents (security, privacy, reliability, performance, architecture, evidence, dependencies, UI/UX, product)
- Batch 1 baseline: `pnpm audit`, `tsc`, `prettier`, `node --test`, CI workflow
- Full swarm: `tools/audit` (`@talos/audit-swarm`) runs all 9 agents locally with zero extra toolchain, writes `docs/reports/audit-latest.{md,json}` plus dated copies
- Severity model: info / warning / critical; criticals fail the command (exit 1), warnings do not (exit 0)
- CI runs the swarm after build; the Tauri bundle itself is excluded (needs WebView2/bundler + display — see CI notes)

## Batch 1 boundary

In scope (delivered, v0.1.0):

- monorepo structure, pnpm, TypeScript strict base, `just` runner
- `.github/workflows/ci.yml` green baseline (Batch 2: 6-job hardened pipeline — see [docs/CI.md](CI.md))
- docs: ARCHITECTURE, PROGRESS, EVIDENCE, ROADMAP (+ Batch 2: CI, media kit, RUNBOOK, SECURITY, CONTRIBUTING)

Out of scope (Batch 1):

- Tauri app code, Rust crates, UI components, RAG, ledger enforcement, 3D UI, full swarm

## Batch 2 boundary (storefront + runtime depth, in progress)

In scope:

- Private GitHub repo with full history + tags, enterprise README/badges, brand/media system, issue/PR templates, CODEOWNERS, community health files
- Deeper agent runtime (queue, lifecycle, cancellation), fs/shell tools behind policy, browser-use foundation, computer-use safety layer, context budgets
- Hardened CI must stay green; Tauri bundle stays local-only with documented reason

Non-goals (unchanged): cloud accounts, external LLM keys in-repo, mobile targets.
