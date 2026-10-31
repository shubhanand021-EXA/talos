# Talos Architecture — Pentagon Architecture

Batch 1 status: **Foundation and Shield baseline only**. Brain, Soul, and Face are specified here but not implemented until later batches.

## 1. Foundation: Tauri 2 + Rust + React

- Desktop shell in `apps/desktop/`
- UI package in `apps/ui/`
- Rust workspace in `crates/`
- Shared TS contracts in `packages/contracts/`
- pnpm workspaces + strict TypeScript + `just` task runner
- Reproducible setup via `just setup` and `just doctor`

Batch 1 scope: monorepo skeleton, tooling, hygiene, CI, shared TS contracts,
Rust workspace with Policy Broker core + SQLite evidence store. No Tauri windows,
no full ledger verification.

## Rust workspace layout (`crates/`)

```text
crates/
  Cargo.toml          # workspace: talos_common, policy_broker, evidence_store
  talos_common/       # TalosError/Result, typed IDs, time + serde + tracing helpers
  policy_broker/      # PolicyEngine trait, DefaultPolicyEngine, talos-policy-broker --selftest
  evidence_store/     # SQLite store (sqlx), migrations/, append-only repo functions
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
- Machine-readable ledger + compensation plans (retry / fallback / escalate) arrive later

Status: **ledger format defined in Batch 1, enforcement in later batches**.

## 4. Face: Void Neon 3D Spatial Topology UI

- AAA-grade desktop UX: dark void, neon graph, spatial topology of agents/tools/memory
- React + Vite + 3D (Batch 4+)

Status: **specified, not implemented**.

## 5. Shield: 9-Agent Audit Swarm

- 9 independent audit agents (security, deps, types, style, docs, evidence, perf, api, license)
- Batch 1 baseline: `pnpm audit`, `tsc`, `prettier`, `node --test`, CI workflow
- Full swarm orchestration arrives in later batches

## Batch 1 boundary

In scope:

- monorepo structure, pnpm, TypeScript strict base, `just` runner
- `.github/workflows/ci.yml` green baseline
- docs: ARCHITECTURE, PROGRESS, EVIDENCE, ROADMAP

Out of scope:

- Tauri app code, Rust crates, UI components, RAG, ledger enforcement, 3D UI, full swarm
