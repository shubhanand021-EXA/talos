# Talos Architecture — Pentagon Architecture

> Repo: `https://github.com/shubhanand021-EXA/talos` (public, default branch `main`; see [ADR 0005](adr/0005-public-repository-transition.md)). Batch 2 storefront: enterprise README, brand/media system (`docs/media/`), hardened 6-job CI ([docs/CI.md](CI.md)). Control-loop diagram: [docs/media/architecture.svg](media/architecture.svg).

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
  Cargo.toml          # workspace: talos_common, policy_broker, evidence_store, agent_runtime, tool_runtime, computer_use, context_budget, tool_rag, workspace_manager
  talos_common/       # TalosError/Result, typed IDs, time + serde + tracing helpers
  policy_broker/      # PolicyEngine trait + DefaultPolicyEngine + context hooks, talos-policy-broker --selftest
  evidence_store/     # SQLite store, migrations (0001 init, 0002 ledger, 0003 runtime queue), append-only repo
  agent_runtime/      # lifecycle state machine + durable queue + worker/supervisor + MockEchoAgent scenarios
  tool_runtime/       # ToolId/Manifest/Invocation/Receipt + Registry + adapters + fs/exec/code/git/browser/computer tools + gated executor
  workspace_manager/  # per-task isolated roots: git worktrees or temp dirs + worktree-aware cleanup
  computer_use/       # safety envelope + virtual-screen mock + disabled real-adapter stub (Rust-authoritative foundation)
  context_budget/     # layered budgets + deterministic TypeCompact + invariant gate (no external calls)
  tool_rag/           # local-first tool retrieval: SQLite index + lexical ranking (no embeddings yet)
```

- Workspace lints forbid `unsafe_code`; binaries commit `Cargo.lock`.
- Policy decisions mirror `@talos/contracts`: allow / deny / require_evidence /
  require_compensation / require_human_review, default deny.
- Quality gates: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`, `talos-policy-broker --selftest`.

## 2. Brain: Tool-RAG + context budgeting + TypeCompact

- Tool registry with retrieval (Tool-RAG) — **live**: `crates/tool_runtime` exposes manifests + JSON schemas; `crates/tool_rag` indexes manifests + receipts + evidence + task/compensation context in SQLite (normalized tables, deterministic tokens) and ranks with a local lexical provider; retrieval ranking beyond baseline arrives later
- Explicit context budgets per turn — **live**: `crates/context_budget` accounts seven layers (safety/system/task/session/evidence/tool/scratchpad) against an 8_000-token budget with a local char-fallback estimator
- TypeCompact: schema-first compaction for long histories — **live**: deterministic TypeCompact preserves IDs, safety constraints, policy decisions, evidence/compensation refs, and schema type names; drops only reference-free segments on discardable layers; every result carries a `sha256:` proof hash and is invariant-gated before persistence

Status: **tool + browser registries, context budgeting/compaction, and Tool-RAG baseline live locally (Batch 2 tools/browser/computer/context/rag); browser live Chromium production-ready behind an explicit opt-in (Batch 3); embedding-backed ranking specified, not implemented**. Target: Batch 3+.

## 2b. Tools: policy-gated execution framework (Batch 2, live)

- `crates/tool_runtime`: `ToolId` / `ToolManifest` (risk, capabilities, reversibility, evidence/compensation needs, JSON schemas) / `ToolInvocation` / `ToolReceipt` (status, policy verdict, evidence/compensation links) + `ToolRegistry` (register/list/get/validate, metadata for UI + RAG)
- Safe built-ins (33): `fs.read` / `fs.list` (bounded, no symlink escape) / `fs.search` (bounded) / `fs.write` (policy + evidence + `backup_restore` compensation before mutation) / `fs.edit` (unified-diff apply, dry-run validated, backup + compensation) / `fs.create` / `fs.delete` (irreversible, backup + compensation) / `fs.move` / `process.exec` (disabled by default, per-invocation opt-in for workspace coding tasks, coder allowlist, denylist, 60s default / 300s max timeout, minimal env, 100KB hashed capture) / `code.run_tests` (auto-detected cargo/pnpm/npm/pytest, bounded hashed output) / `code.install_deps` (manifest-aware, network-gated) / `git.status|diff|branch` (read-only Low, workspace-scoped) / `git.commit` (Medium + evidence, message validated) / 9 `browser.*` tools (isolated profiles, DOM-first safety, mock deterministic, production live Chromium behind `TALOS_BROWSER_LIVE=1`) / 9 `computer.*` tools (virtual screen, safety envelope, mock-only)
- Coding workspaces (`crates/workspace_manager`, Batch 3): per-task isolated roots — git worktree (`-b talos/<task>`) when a repo is hinted, else temp dir; workspace root is the only filesystem root for the task's tools; cleanup removes the worktree registration + dir; file/shell/git/code tools resolve under it with symlink-escape rejection intact
- Adapters: `DryRun` (default for risky tools, no side effects) / confined local execution / future `Sandbox` placeholder (honest unavailable)
- Policy: every invocation becomes an `ActionRequest` (capability + risk from manifest); write/exec need evidence; browser mutations need evidence (dry-run first); computer reads run directly while mutations need evidence and High-risk actions need human review; high-risk flags escalate, never downgrade; irreversible-equivalent needs compensation; denials produce structured codes (`policy_denied`, `tool_disabled`, plus safety codes)
- Evidence: every receipt persisted as `tool_receipt` evidence (verified), including denials; transitions journaled
- Desktop: `list_tools`, `describe_tool`, `invoke_tool_dry_run`, `invoke_tool`, `computer_safety_status`, `computer_emergency_stop`, `computer_reset_stop` (17 commands total, generic across tools); UI Tools page (`/tools`) + Browser page (`/browser`) + Computer page (`/computer`) with catalogs, dry-run forms, receipt viewers
- Architecture decisions: [docs/adr/0001-browser-use-architecture.md](adr/0001-browser-use-architecture.md) (Rust authoritative, TS worker + opt-in live adapter), [docs/adr/0002-computer-use-safety-layer.md](adr/0002-computer-use-safety-layer.md) (Rust-first, mock-only, disabled by default, emergency stop)

## 2b-i. Live browser worker (Batch 3, production)

- `packages/browser-use` (Playwright 1.64.1, optional peer): `LiveBrowserAdapter` executes all 8 actions against real Chromium — `launchPersistentContext` per task profile (headless default, `--no-first-run --disable-default-apps --disable-extensions`, downloads rejected), selector resolution + visibility/interactable checks before every mutation, `domcontentloaded` navigations, masked password regions, content-addressed screenshot artifacts (`<5 MiB`, skipped on sensitive pages), bounded JPEG thumbnails, NDJSON/SSE-free direct CDP-less Playwright calls, structured timeout/not-found/not-visible/navigation errors without page-content leakage.
- Safety gates (worker + Rust, neither bypassable): live requires `TALOS_BROWSER_LIVE=1` (else mock/disabled); mutations default to dry-run; high-risk needs explicit `approved: true` (`approval_required` denial + `browserAuthRequired` event otherwise); password fields never auto-filled (`auth_required`); navigation restricted to `http(s)` with scheme blocklist (`file/chrome/about/javascript/data`) and optional origin allowlist (`TALOS_BROWSER_ALLOWED_ORIGINS`); sensitive domains (`TALOS_BROWSER_SENSITIVE_DOMAINS`) suppress artifact storage.
- Sessions: `LiveSessionManager` (max 1 warm by default, 5-minute idle auto-close, `session.launch|status|close|close_all` over JSON-RPC `--serve` or single-shot argv/stdout); `browser.session` tool exposes it through the generic IPC; emergency-stop path closes all warm sessions.
- Rust: `tool_runtime::browser_worker` spawns `node worker.js` per live call (30s kill timeout, stdout-parsed receipt, `browser_worker_crashed/timeout/protocol` structured errors); executor routes live browser arms to the worker only when opted in, mock otherwise; crashes emit `browserWorkerCrashed` and mark the task failed with ledgered evidence. Raw keys, prompts, and page content never cross into evidence — only hashes, metadata, and bounded extracts.

## 2c. Computer-use: safety-first foundation (Batch 2, live mock)

- `crates/computer_use`: `ComputerAction` (9) / `ComputerTarget` (coords, bounds, app/window hints, confidence) / `ComputerReceipt` (screenHash, boundingBox, confidence) / `SafetyEnvelope` (global enable flag off, dry-run default, 30/min rate limit, 1440x900 bounds, 0.7 confidence floor, dangerous zones, e-stop latch) / `MockComputer` (virtual screen, seeded elements, deterministic hashes) / `RealComputerAdapter` trait + disabled stub + `real-os-control` compile-error gate
- Safety envelope runs before every adapter: stop latch blocks everything (even dry-run); disabled flag blocks live; rate limit caps live actions; bounds/zones/confidence gate both modes; warnings surface in receipts
- Policy: read-only computer tools (Low + UiAction) run directly; move/scroll (Medium) need evidence first; mutations (High, non-reversible) require human review; highRisk flag escalates, never downgrades; denials carry structured codes (`emergency_stop`, `computer_disabled`, `rate_limited`, `out_of_bounds`, `dangerous_zone`, `low_confidence`)
- Desktop: `computer_safety_status`, `computer_emergency_stop` (latches + cancels active queue tasks + emits `computerSafetyChanged`), `computer_reset_stop` (20 commands total with context); UI Computer page (`/computer`) with safety panel, mock screen visualization, action builder, dry-run/live buttons, receipt viewer, e-stop controls; status bar shows STOPPED/armed/disabled

## 2d. Context budgeting + TypeCompact (Batch 2, live foundation)

- `crates/context_budget`: `ContextLayer` (7, priority-ordered) / `BudgetConfig` (8_000 total; safety 800, system 800, task 1600, session 1200, evidence 1600, tool 1200, scratchpad 800) / `BudgetReport` (per-layer usage, overflow flags) / `CompactionResult` (original/compacted estimates, preserved invariants, dropped/trimmed lists, `sha256:` proof hash) / `CompactionError` (`invariant_*` codes)
- Estimator: local char-fallback `ceil(chars/4)`, no external calls, deterministic across platforms
- TypeCompact rules: protected/safety-critical/safety segments verbatim; drops only reference-free segments on discardable layers (lowest priority first); dedupe repeated lines then trim tail with explicit `[…trimmed {n} chars…]` marker; invariant gate rejects any ID/evidence/compensation/policy/constraint/schema/safety loss before persistence
- Desktop: `context_budget_status` (read-only), `context_compact` (compact + persist verified `context_compaction` evidence + emit `contextCompacted`), `context_diagnostic_snapshot` (diagnostic loop → snapshot with claim/plan refs → compact → persist → emit) — 21 commands total with Tool-RAG; UI Context page (`/context`) with budget bars, compacted preview, invariant checklist, receipt panel, protected items list; architecture decision: [docs/adr/0003-context-budget-typecompact.md](adr/0003-context-budget-typecompact.md)

## 2e. Tool-RAG: local-first routing foundation (Batch 2, live baseline)

- `crates/tool_rag`: `ToolQuery` (intent 1..=1024, capability allow-list, risk ceiling, evidence requirement, top-K 1..=20) / `ToolCandidate` (tool id, score, reason, matched fields, risk level, required approvals) / `LocalLexicalProvider` (field weights name 3.0 / capability 2.5 / description 1.0 / text 0.5) / `DisabledEmbeddingProvider` (fails closed, no external API) / SQLite index (`rag_tools`, `rag_terms`, `rag_receipts`; normalized tables, deterministic tokens — FTS5 evaluated and rejected)
- Ranking: filters first (disabled never rank; capability allow-list; risk ceiling `low<medium<high<irreversible`; evidence requirement = manifest flag or a successful receipt), then keyword + capability bonus + success boost (capped) + evidence boost − failure/denial penalty (capped, floor 0); ties break by tool id; every candidate self-explains
- Index sources folded deterministically: manifests + tool-receipt outcomes + task titles + evidence kinds + compensation strategies (for tools that require them)
- Desktop IPC: `query_tool_candidates` (read-only; rebuilds the index from registry + ledger, ranks locally; 21 commands total); UI Tool-RAG panel on the Tools page (intent box, capability/risk filters, evidence toggle, top-K, candidate list with score/reason/risk badge/approvals, click-through to the manifest); architecture decision: [docs/adr/0004-tool-rag-foundation.md](adr/0004-tool-rag-foundation.md)

## 2f. LLM gateway: BYOK model routing (Batch 3, live foundation)

- `crates/talos_secrets`: OS-keychain vault (`com.talos.desktop`, `provider.{id}.api_key`) behind a `SecretBackend` trait + hermetic mock; values travel only into the HTTP `Authorization` header, never into IPC/events/evidence/logs/errors.
- `crates/llm_gateway`: `LlmProvider` trait (`complete`/`stream` + capability flags); OpenAI (chat completions + SSE), Anthropic (messages API + SSE), Ollama (`/api/chat` + NDJSON, `/api/tags` discovery, no key); injectable `HttpClient` (live reqwest, mock for tests).
- `LlmGateway` flow: validate → resolve adapter → budget check (`usage_records` sum vs `runtime_tasks.max_spend_usd`, `None` = uncapped) → record `llm.request` evidence → evaluate `NetworkRequest` (Rule 5e: allow with evidence; high-risk escalates; anonymous calls denied) → execute with backoff retry → persist `usage_records` + `model_interactions` (hashes only) → receipt with estimated cost.
- Registry: hardcoded reviewable `ModelDescriptor` catalog (list prices) + cheapest-adequate `ModelRouter` (tools/vision/rate/provider filters); Ollama discovery merges at query time.
- Runtime: `agent_runtime::LlmAgent` routes through the gateway when a key is configured, deterministic mock fallback otherwise (echo loop untouched).
- Desktop: secrets vault + gateway in `AppState`; 6 IPC commands (`listProviders`, `listModels`, `testProviderConnection`, `getUsageSummary`, `storeProviderKey`, `deleteProviderKey`); 3 events (`llmRequestStarted`, `llmRequestCompleted`, `llmBudgetWarning` at 80% of cap).
- UI: Settings → Model providers (lazy-loaded panel: status, Add/Test/Remove key, model catalog, session usage); bridge validates every response with `llm.ts` Zod schemas.
- Architecture decision: [docs/adr/0006-llm-gateway-byok.md](adr/0006-llm-gateway-byok.md) (BYOK default, keychain-only keys, policy Rule 5e, hash-only evidence).

## 2g. Context Assembly Engine (Batch 3, live foundation)

- `crates/context_assembler`: `Config` (`.talos/context.config.json`: 20/25/35/10/10 allocation, 15% output reservation, `ctx-assembler/1` template; validated to sum to 100), `Estimator` (tiktoken `cl100k` for OpenAI-family with char/4 fallback, identity stamped per assembly), `Gatherer` (task contract, verified-first evidence, memory claims, session events, chain head, Tool-RAG rank over the live index), deterministic versioned `Templates`, and `assemble` (per-layer TypeCompact on overflow, truncation fallback with degraded flag, SHA-256 assembly hash).
- Safety rules stay verbatim and are never compacted; invariant-gate failures degrade to truncation, never to silent drops. Over the hard model limit after all compaction → `HardLimitExceeded`, surfaced as `context_budget_exceeded` + `contextBudgetExceeded` event so the task fails gracefully.
- Runtime: `agent_runtime::LlmAgent::execute_task_with_assembly` sends assembled messages through the gateway (mock fallback preserved when unconfigured).
- Desktop: `context_config` in `AppState`; `preview_context` (gather → assemble → record `context_assembly` metadata evidence → metadata-only DTO), `get_context_config`, `update_context_config` (validated); `CONTEXT_BUDGET_EXCEEDED` event.
- UI: Context page → Assembly Preview (lazy panel section: per-layer allocated-vs-used bars, tools included, evidence refs count, compactions with proof hashes, invariant checklist, template version, estimator, assembly hash, Reassemble button).
- Docs: ADR pattern follows [docs/adr/0006-llm-gateway-byok.md](adr/0006-llm-gateway-byok.md) (BYOK + policy-gated + hash-only evidence).

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

Status: **ledger + compensation foundation live locally; durable runtime queue (0003) persists entries, attempts, transitions, cancellations with `task.transition` ledgering; every tool receipt persisted as `tool_receipt` evidence; file-backed DB path arrives later**.

## 4. Face: Void Neon 3D Spatial Topology UI

- AAA-grade desktop UX: dark void, neon graph, live operational topology
  (React + Vite + R3F, Batch 2 live): core + agents + tasks + tools +
  browser/computer-use nodes, policy gate + evidence ledger + audit
  sentinels; task-assignment, invocation, policy, and evidence edges with
  state colors; queued dim pulse, executing flow, amber policy gate, red
  denied/failed, green completed fade, gray canceled, red e-stop overlay
- Interaction: click-select → inspector (cancel/retry task, view
  receipt/evidence, view manifest), hover tooltips, focus mode, kind
  filters, lightweight event replay slider; recent-activity glow, evidence
  and policy pulses
- Performance: deterministic builder with task/tool caps, shared
  materials/geometries, no per-frame allocation, DPR caps, reduced-motion
  support, lazy-loaded viewport, live FPS meter; fallback + error
  boundary keep the dashboard usable without WebGL
- HUD: active/waiting task counts, browser/computer event counts,
  computer-use safety pill, ledger health + evidence version, e-stop
  indicator, quality switch, FPS readout

Status: **live locally; `docs/media/hero.gif` regenerated with the new topology glyphs**.

## 5. Shield: 9-Agent Audit Swarm

- 9 independent audit agents (security, privacy, reliability, performance, architecture, evidence, dependencies, UI/UX, product)
- Batch 1 baseline: `pnpm audit`, `tsc`, `prettier`, `node --test`, CI workflow
- Full swarm: `tools/audit` (`@talos/audit-swarm`) runs all 9 agents locally with zero extra toolchain, writes `docs/reports/audit-latest.{md,json}` plus dated copies
- Batch 2 hardening: safety-default verification (tool/browser/computer execution gates, human-review gating), performance baselines (`.talos/baselines.json`) with WARN on regression and FAIL on critical regression, deterministic run metrics in every report, live dashboard on the Audit page, CI uploads reports + baselines
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

- Public GitHub repo with full history + tags, enterprise README/badges, brand/media system, issue/PR templates, CODEOWNERS, community health files
- Deeper agent runtime (queue, lifecycle, cancellation), fs/shell tools behind policy, browser-use foundation, computer-use safety layer, context budgets
- Hardened CI must stay green; Tauri bundle stays local-only with documented reason

Non-goals (unchanged): cloud accounts, external LLM keys in-repo, mobile targets.
