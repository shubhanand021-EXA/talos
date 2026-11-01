# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased] — Batch 4 Prompt 01 "The Hardening": durable persistence

Replaces the ephemeral in-memory SQLite database with file-backed durable
state: every task, evidence record, policy evaluation, and cost record now
survives app restarts and crashes.

### Added

- File-backed `EvidenceDb::connect_file` (WAL, foreign keys, 5s busy timeout,
  idempotent migrations + `schema_version` mirror); `connect_memory` kept for
  tests only; read-then-write chain appends use `BEGIN IMMEDIATE`.
- App-data layout (`<app_data>/talos.db`, `backups/`, `models/`) with
  `TALOS_DATA_DIR` override; startup logs the resolved path (path only).
- Startup crash recovery: chain verification (broken chains reported, never a
  refusal) + interrupted-task reconciliation (`interrupted_by_restart`,
  `approval_lost_on_restart`, `requeued_after_restart`).
- Backup (`VACUUM INTO`, newest-5 retention) + transactional restore with
  schema-match validation and post-restore chain verification.
- Desktop: `AppState::new_persistent` (Tauri `app_data_dir`), 4 storage IPC
  commands (`get_storage_info`, `create_backup`, `restore_backup`,
  `list_backups`) + 3 events (`backupCreated`, `backupRestored`,
  `integrityCheckCompleted`); contracts `storage.ts`; Settings → Storage
  panel (path, sizes, schema, integrity, backup list, Create + confirm-gated
  Restore) + status-bar storage pill.
- Tests: 35 `evidence_store` (WAL proof, idempotent migrations, close/reopen
  survival, chain survival, backup/restore round-trip, recovery transitions,
  tamper-after-restart, concurrent writers), 62 desktop (incl. recovery
  terminal-set parity + in-memory refusal), 73 contracts, 124 UI.

### Changed

- `docs/SECURITY.md`: the "in-memory database" caveat now describes
  file-backed storage without at-rest encryption (filesystem access reads
  `talos.db`; full-disk encryption is the current mitigation).
- `docs/ARCHITECTURE.md`: new Persistence Layer section (§2h); Batch 1
  leftovers and Soul status updated.

## [0.3.0] — 2026-11-01 — Batch 3 "The Awakening" release

Batch 3 turns Talos from a safe local foundation into a working autonomous
system: real model calls, assembled context, live browser execution,
multi-agent delegation, isolated coding workspaces, semantic memory,
human-in-the-loop approvals, a real computer-use driver, and full
observability — proven end-to-end by the deterministic
`proof-of-awakening` demo (`pnpm demo:awakening` / `just demo-awakening`).
See `docs/evidence/batch-3-final-report.md`.

### Added (Batch 3 rollup, Prompts 01–09 + Awakening)

- Real LLM gateway with BYOK keychain (Prompt 01): OpenAI/Anthropic/Ollama providers, OS-keychain secrets, policy-gated + budget-capped calls, usage/hash ledger, Settings → Model providers.
- Context assembly engine (Prompt 02): task-scoped gathering, Tool-RAG-ranked catalogs, TypeCompact budgets, `preview_context` metadata API.
- Live browser execution (Prompt 03): Playwright worker behind `TALOS_BROWSER_LIVE=1`, mock by default, ledgered receipts.
- Multi-agent supervisor delegation (Prompt 04): deterministic decomposition, fan-out/depth/authority validation, child execution, artifact merge, task tree UI.
- Filesystem + shell workspace isolation (Prompt 05): per-task roots (git worktree or temp dir), coder allowlist, network-gated installs, cleanup.
- Semantic memory (Prompt 06): local 384-dim embeddings, SQLite vector store, incremental indexer, hybrid recall, skill proposals.
- HITL approvals (Prompt 07): Plan/Act/Auto modes, ApprovalSheet with diff viewer, batch low-risk, compensation execution, expiry.
- Real computer-use driver (Prompt 08): accessibility APIs behind the `real-os-control` feature flag (never CI/default), vision verification loop, session isolation.
- Observability (Prompt 09): `talos_telemetry` spans (17 sites, local-first, privacy allowlist), `cost_tracker` real pricing + budgets, `cost_records` ledger, 5 operations IPC commands, `/operations` dashboard, 10th audit agent + baseline gates.
- Proof of Awakening: `crates/awakening_demo` (13-step flow: submit → decompose → delegate/merge → browser research → workspace → implement → test → assemble → recall → LLM route → approve → meter → verify) with full-mock (CI-safe), llm-real, browser-real, and full-real modes.
- Versions cohere at 0.3.0 across workspace crates, desktop shell, UI, contracts (`CONTRACTS_VERSION`), and tooling manifests.

### Changed

- README: "The Awakening" milestone, real-LLM quick-start, provider guide, safety-model explanation, updated architecture + capability matrix.
- Audit swarm is now 10 agents (Telemetry joins); `.talos/baselines.json` gains telemetry gates.

## [0.2.0] — 2026-10-31 — Batch 2 integration release

Batch 2 ships as one coherent, local-first system: durable runtime, policy-gated tool framework, browser/computer-use safety foundations, context budgeting with TypeCompact, local-first Tool-RAG, live Void Neon topology, and a hardened audit swarm — verified end-to-end by the deterministic `batch2-integration-demo` (`pnpm demo:batch2` / `just demo-batch2`). See `docs/evidence/batch-2-final-report.md`.

### Added (Batch 2 rollup)

- Durable agent runtime: SQLite-backed queue, 10-state lifecycle, priorities, timeouts, retries, cancellation, supervisor, MockEchoAgent scenarios, 5 IPC commands, live queue UI.
- Tool framework: `crates/tool_runtime` (22 built-ins, gated executor, evidence receipts, compensation plans), 4 IPC commands, Tools page with dry-run forms and Tool-RAG panel.
- Browser-use foundation: isolated mock browser (opt-in live Chromium), 8 policy-gated tools, Browser page.
- Computer-use safety layer: safety envelope (disabled by default, e-stop), 9 mock-only tools, Computer page with safety panel and mock screen.
- Context budgeting + TypeCompact: 7-layer budgets, deterministic compaction with proof hashes and invariant gates, Context page, 3 IPC commands.
- Tool-RAG: SQLite lexical index over manifests/receipts/evidence/tasks/compensation, risk ceilings, receipt-history scoring, read-only `queryToolCandidates`.
- Void Neon live topology: 9 node kinds, flow edges, filters/focus/replay, inspector actions, HUD upgrades, e-stop overlay.
- Audit hardening: execution-default verification, lifecycle/cancellation suites, `.talos/baselines.json` performance gates, live Audit dashboard, CI report + baseline artifacts.
- Versions cohere at 0.2.0 across workspace crates, desktop shell, UI, and contracts (`CONTRACTS_VERSION`).

### Changed

- Dashboard counts distinguish mock vs live sources; task queue shows an empty state.
- All 6 workspace manifests declare `license: MIT` (matches LICENSE + crates).
- `hero.gif` regenerated with the new topology glyphs (deterministic generator).

## [Unreleased]

### Added (Batch 3 — Semantic vector search with local embeddings)

- `crates/embedding_engine` (new): deterministic offline embeddings — 384-dim hashing-trick backend (`local-hash-384/1`, L2-normalized) + corpus-fitted TF-IDF vectorizer, cosine similarity, `.talos/models/` probe reporting the hash fallback (no ONNX runtime bundled; ADR 0007); 7 tests.
- `crates/vector_store` (new): SQLite vector tables in the shared `EvidenceDb` (`vector_entries` + `memory_index_state`); `upsert_entry` (embed + content-hash dedupe — unchanged rows skip re-embedding), filtered brute-force cosine `search` (source type, task, date range, verified-only), `remove_by_source`, counts, clear; <100ms top-10 over 1k rows; 6 tests.
- `crates/evidence_store`: migration `0006_skills.sql` (`skills` quarantined→validated→published, no skips); `create/get/list/update_skill_status`; existing migrations untouched.
- `crates/memory_indexer` (new): `index_incremental` (claims, verified-first evidence, completed tasks, skills; proposes quarantined skills from completed tasks with verified claims, deduped by source task), `rebuild_index`, hybrid `hybrid_search` (scope filter → lexical + semantic → normalized blend → recency/verified rerank → content-hash dedupe → top-K with source refs and scores), `RetrievalMode::{Lexical,Semantic,Hybrid}`, `spawn_background_indexer`; 8 tests.
- `crates/tool_rag`: `RetrievalMode` + `parse_retrieval_mode` + `rank_with_semantic` (max-normalized blend, empty map ≡ legacy `rank`) + `semantic_text`; `LocalLexicalProvider` default unchanged.
- `crates/context_assembler`: `semantic_segments` maps hybrid hits into session-layer segments competing in the existing layer budget; `preview_context` gathers top-5 hybrid recall and skips already-gathered evidence.
- Desktop: `memory.rs` IPC (`search_memory`, `get_index_status`, `rebuild_index`, `list_skills` + `memoryIndexed`, `skillCreated`, `indexRebuilt` events; search indexes incrementally first, then recalls); `query_tool_candidates` gains optional `mode` (lexical default; semantic/hybrid embed intent + docs and blend); 60s background indexer in `AppState`; 39 IPC commands total.
- Contracts: `memory.ts` (`SearchResult`, `MemoryQuery`, `IndexStatus`, `Skill`, event payloads), 4 commands + 3 events; `system-ipc` gates updated; new `memory.test.ts`.
- UI: `/memory` route + nav + palette; `MemoryPage` (index status + rebuild, hybrid search with source/mode/verified filters, scored result cards, quarantined→published skill registry); Memory link on Evidence page; `useTalosSync` memory events; 5 page tests.
- Tests: 7 `embedding_engine`, 6 `vector_store`, 8 `memory_indexer`, 3 new `tool_rag` (+1 desktop hybrid test), 62 contracts, 103 UI.

### Added (Batch 3 Prompt — Isolated coding workspace with shell and git)

- `crates/workspace_manager` (new): per-task isolated roots — git worktree (`git worktree add <base>/<task_id> -b talos/<task_id>`) when a repo is hinted, otherwise an isolated temp dir; `WorkspaceManager::{create_workspace, ensure_workspace, get_workspace, list_workspaces, cleanup, workspace_root, is_within}`; branch sanitization; worktree-aware cleanup (`worktree remove --force` then dir removal); 5 tests.
- `crates/tool_runtime` (23 → 33 built-ins): structured file editing (`fs.edit` unified-diff apply with dry-run validation + backup, `fs.create`, `fs.delete` irreversible + backup, `fs.move`), test runner (`code.run_tests`: auto-detects cargo/pnpm/npm/pytest, bounded output, `TestResult` artifact with content hash), dependency installer (`code.install_deps`: manifest-aware, network-gated via `allow_network`), git integration (`git.status/diff/branch` read-only Low, `git.commit` Medium + evidence, all workspace-scoped with `--git-dir`/`--work-tree`/`-C` rejection).
- Shell upgrade: `process.exec` now High + reversible (evidence-gated instead of human-review), timeout 100..300000ms (default 60s), minimal env (PATH/SystemRoot/LANG only, no secrets), argv-exact spawn with cwd locked to the workspace root, 100KB-per-stream capture with truncation markers + SHA-256 hashes + `durationMs` + `workspacePath` (`ShellReceipt`); denylist (`sudo`, `rm -rf /`, `chmod 777`, `del /f`, `curl|wget` piped to shell, `--no-preserve-root`) checked before the allowlist; `coder_exec_allowlist` (git/cargo/npm/pnpm/node/python/pytest/…).
- Policy mapping: shell High + evidence, file delete Irreversible + compensation (executor pre-records the `backup_restore` plan on the RequireCompensation verdict and re-evaluates), git commit Medium + evidence, install Medium + evidence + network gate, test run Low + evidence; executor compensation covers `fs.write/edit/create/move/delete`.
- Desktop: `AppState.workspaces` (`talos-workspaces` base dir); per-task `tool_config_for` (workspace root + coder allowlist + network on for workspace tasks, shared root + exec disabled otherwise); `process.exec` per-invocation opt-in (manifest default stays disabled); `workspace.rs` IPC (`create_workspace`, `get_workspace`, `list_workspaces`, `cleanup_workspace`, `get_workspace_tree` + `workspaceUpdated` event); delegation spawns coder workspaces on child creation and cleans the subtree on cancel; 35 IPC commands total.
- Contracts: 10 new `ToolId`s, `workspace.ts` (`WorkspaceInfo`, `ShellReceipt`, `TestResult`, `WorkspaceTreeNode`, `WorkspaceUpdatedPayload`), 5 commands + 1 event; tests updated (`tool.test` 33 ids, `system-ipc` gates, new `workspace.test.ts`).
- UI: `/code` route + nav + command palette entry; `CodeWorkspacePanel` (workspace create/load/cleanup, file tree, shell dry-run/live with hash receipts, test runner with pass/fail, diff viewer with add/remove highlighting, git status, workspace list); Tools page templates for all 10 new tools; `useTalosSync` `workspaceUpdated` feed; 6 panel tests.
- Tests: 62 `tool_runtime` (patch apply/reject, create/move/delete round trip, delete compensation, git scoping/gates, test detection + counts, install network gate, shell hashes), 5 `workspace_manager`, 48 desktop (incl. workspace lifecycle), 58 contracts, 98 UI.

### Added (Batch 3 Prompt 04 — Supervisor multi-agent delegation)

- `crates/evidence_store`: migration `0005_delegation.sql` (`runtime_tasks.parent_task_id` + index); `RuntimeTaskRow.parent_task_id`, `EnqueueParams.parent_task_id`, `list_children`, `task_depth` (roots = 1), `get_task_tree` (boxed recursion), `TaskTreeNode`.
- `crates/agent_runtime`: `delegation` (protocol types, `MAX_FANOUT=4`/`MAX_DEPTH=2`, deterministic `decompose_task` code/browser/research, `is_narrower_authority`, `validate_delegation`, `split_budget_equal`, `merge_results`); `specialists` (`Specialist` trait + `CoderAgent`/`BrowserAgent`/`ResearchAgent` scoped allowlists, 4000-token window, structured `ArtifactRef`s); `messages` (typed `SupervisorToAgent`/`AgentToSupervisor` + bounded `DelegationBus`); `concurrency` (`ConcurrencyController`, file-lock + browser-session exclusivity, `ConflictKind`); `supervisor::SupervisorAgent` (decompose/plan/validate/policy-gate `SystemControl`/medium/merge/retry-vs-escalate, `parent_authority`); `queue` (`TaskView.parent_task_id`, `submit_task_with_id`, `TaskTreeView`, `get_task_tree`, `cancel_subtree`); new `RuntimeEvent` variants (`agentDelegated`, `childTaskSpawned/Completed/Failed`, `resultsMerged`, `conflictDetected`).
- Policy: spawning requires `SystemControl`/medium evaluation with live ledger context (auto-records `delegation_scope` evidence, then re-evaluates; never overrides a deny); children independently policy-gated; budgets split equally.
- Desktop: `apps/desktop/src-tauri/src/delegation.rs` (`delegate_task`, `get_task_tree`, `cancel_subtree` + `DelegateTaskOutput`); 5 events (`agentDelegated`, `childTaskSpawned/Completed/Failed`, `resultsMerged`); 33 IPC commands total.
- Contracts: `delegation.ts` (role/artifact/request/result/input schemas), `TaskQueueEntry.parentTaskId`, `TaskTreeNode`, 3 commands + 5 events; `system-ipc.test.ts` gate updated; new `delegation.test.ts`.
- UI: `TaskTree.tsx` (tree + forest, `forestFromQueue`), Agents page Supervisor panel (delegate, load tree, cancel subtree, specialist roster), `RightInspector` delegation details + subtree actions, `useTalosSync` delegation events, Void Neon parent → child delegation edges, topology delegation test.
- Tests: 51 `agent_runtime` (decompose/fan-out/narrowing/bus/merge/conflict/tree/cancel/depth), 4 desktop delegation (coder/multi/grandchild-reject/cancel-subtree), 55 contracts, 92 UI.

### Added (Batch 3 Prompt 03 — live Playwright browser engine)

- `packages/browser-use`: production `LiveBrowserAdapter` (Playwright 1.64.1 optional peer) — all 8 actions with selector resolution, visibility checks, masked password regions, content-addressed artifacts, bounded thumbnails, structured errors; `LiveSessionManager` (max 1 warm, 5-min idle close, `session.*` JSON-RPC); scheme/origin/sensitive/auth guards; worker single-shot + `--serve` JSON-RPC with `browserAuthRequired`/`browserWorkerCrashed` events and a 30s per-call budget.
- `crates/tool_runtime`: `browser_worker` spawns the Node worker per live call (30s kill timeout, structured crash/timeout/protocol errors); 8 live arms route to the worker only under `TALOS_BROWSER_LIVE=1`, mock otherwise; new `browser.session` tool (Low, reversible) for session status/launch/close; `approved`/`thumbnail`/`allowedOrigins` params with strict validation.
- Desktop: `browserWorkerCrashed` event emitted on worker crash/timeout; UI Browser page gains Live Mode toggle, session panel, Take Screenshot / Close Session, live timeline, thumbnails, sensitive-page banner, crash banner.
- CI: `browser-live-smoke` (manual `workflow_dispatch` only; installs Chromium, asserts one real navigation); default matrix stays mock-only and network-free.
- Browser-use README live-mode and safety-model docs.

### Added (Batch 3 Prompt 02 — Context Assembly Engine)

- `crates/context_assembler`: budget allocation from `.talos/context.config.json` (20/25/35/10/10, 15% output reservation, validated to sum to 100); token estimator (tiktoken `cl100k` for OpenAI-family, char/4 fallback, identity stamped); gatherer (task contract, verified-first evidence, memory claims, session events, chain head, Tool-RAG ranking); deterministic versioned templates (`ctx-assembler/1`); per-layer TypeCompact on overflow with truncation fallback (safety never compacted); SHA-256 assembly hash; `HardLimitExceeded` graceful failure.
- Runtime: `LlmAgent::execute_task_with_assembly` sends assembled messages through the gateway (mock fallback preserved); over-window assemblies refuse before any provider call.
- Evidence: `context_assembly` metadata rows (structure/counts/hashes, never prompt text).
- Contracts `context.ts` (allocation/config/assembly metadata schemas) + 3 IPC commands (`previewContext`, `getContextConfig`, `updateContextConfig`) + `contextBudgetExceeded` event.
- Desktop: `context_config` in `AppState`; preview (gather → assemble → record → metadata DTO), config get/update; `CONTEXT_BUDGET_EXCEEDED` event at hard-limit failure.
- UI: Context page Assembly Preview (lazy panel section: per-layer allocated-vs-used bars, tools included, evidence refs, compactions with proof hashes, invariant checklist, template/estimator/hash, Reassemble button).
- `crates/context_assembler/README.md`.

### Added (Batch 3 Prompt 01 — LLM gateway with BYOK)

- `crates/talos_secrets`: OS-keychain vault (`com.talos.desktop`, `provider.{id}.api_key`) with `SecretBackend` trait + hermetic mock; store/get/delete/list (names only); values never leave the vault except into the HTTP `Authorization` header.
- `crates/llm_gateway`: `LlmProvider` trait (`complete`/`stream` + capability flags); OpenAI (`gpt-4o`, `gpt-4o-mini`, `o3-mini`), Anthropic (`claude-sonnet-4-20250514`, `claude-3-5-haiku`), Ollama (dynamic `/api/tags` discovery); injectable HTTP client; SSE/NDJSON streaming; backoff retry for transient failures; structured errors (auth/rate-limit/timeout/budget/policy/secret).
- Registry + routing: hardcoded `ModelDescriptor` catalog with list prices, cheapest-adequate `ModelRouter` (tools/vision/rate/provider filters).
- Policy integration: new Rule 5e allows low/medium-risk `NetworkRequest` with a recorded request receipt; budget enforced from `usage_records` vs `runtime_tasks.max_spend_usd` before any traffic.
- Evidence: migration `0004_llm_gateway.sql` (`usage_records`, `model_interactions` with hashes only, task spend-cap column + indexes).
- Contracts `llm.ts` (provider/model/usage/receipt/summary schemas) + 6 IPC commands + 3 events (`llmRequestStarted`, `llmRequestCompleted`, `llmBudgetWarning`).
- Desktop: secrets vault + gateway in `AppState`; `listProviders`, `listModels`, `testProviderConnection`, `getUsageSummary`, `storeProviderKey` (never echoes), `deleteProviderKey`.
- UI: Settings → Model providers (lazy-loaded: status, Add/Test/Remove key via password field, model catalog, session usage, no key ever displayed).
- Runtime: `agent_runtime::LlmAgent` (gateway when a key is configured, deterministic mock fallback otherwise).
- ADR `docs/adr/0006-llm-gateway-byok.md`; crate READMEs.

### Changed (HF-03 public repository normalization)

- Repository is public (Boss decision, effective immediately; ADR `docs/adr/0005-public-repository-transition.md` supersedes all prior "private until launch" statements).
- README: live dynamic badges (CI, release/version, license) plus stars/forks; private-repo note removed.
- Docs (`docs/CI.md`, `docs/media/README.md`, `SECURITY.md`, `SUPPORT.md`, `docs/PROGRESS.md`, `docs/EVIDENCE.md`, `docs/ARCHITECTURE.md`, `docs/ROADMAP.md`, `.github/workflows/README.md`, `CONTRIBUTING.md`) updated for public status; absolute `/.github/` links fixed to relative.
- CI normalized for public runners: desktop compile check on per-push CI, caching + concurrency + docs-only path filters + artifact retention caps + timeout limits retained.
- Previous private-repo compute-exhaustion history recorded neutrally ("private included compute exhausted; resolved by visibility change"); billing/spending/payment topics untouched (out of scope).

### Added

- Audit swarm hardening: execution-default verification (tool/browser/computer gates via live registry tests), human-review gating check, agent-runtime lifecycle + cancellation suite execution with timing, browser profile-isolation documentation check, tool-receipt + TypeCompact receipt linkage checks, migration presence, manifest license compatibility, critical-action reachability, emergency-stop visibility, async-page state affordances, README feature coverage.
- `.talos/baselines.json`: measured performance gates (dist bytes, main-js gzip, void-neon chunk gzip, cargo subset durations, startup probe, swarm total) with WARN/FAIL regression policy; reference test counts and build approximations recorded but never compared.
- Runner: deterministic metric collection (`ctx.record`), post-run baseline comparison merged into the performance agent, Baselines table in Markdown, `metric=value` log summary, best-effort dev-server report copy (`apps/ui/public/reports/`, gitignored).
- Audit dashboard: live report when served, committed snapshot otherwise; summary counts, 9-agent status grid with durations, baseline gate table, report path link; snapshot-sync guard against dashboard drift.
- CI: `audit` job uploads `talos-audit-reports` + `talos-audit-baselines` and prints a baseline summary that fails on critical regressions.

## [Unreleased] — Batch 2 void-neon

### Added

- Live operational topology: 9 node kinds (core/agents/tasks/tools/browser/computer-use/policy/evidence/audit) with state-mapped colors (queued dim, executing flow, waiting-policy amber, denied/failed red, completed green fade, canceled gray, e-stop red overlay).
- Flow edges: task assignment, tool invocation, policy gating, evidence recording — each with live states (active/pending/blocked/completed/failed).
- Interaction: click-select → inspector, hover tooltips, focus mode, kind filters, lightweight event-replay slider; inspector actions (cancel/retry task, view receipt/evidence, view manifest).
- Temporal awareness: recent-activity glow (60s window), evidence/policy pulses, replay scrubber over task-scoped events.
- Performance: deterministic builder with task/tool caps, shared materials, no per-frame allocation, DPR caps, reduced-motion support, lazy viewport, FPS meter; error boundary + WebGL fallback.
- HUD: active/waiting task counts, browser/computer event counts, computer-use safety pill, ledger health + evidence version, e-stop indicator.
- Contracts: `SceneNodeKind` extended (tool/browser/computer-use/policy/audit); `hero.gif` regenerated with the new topology glyphs (deterministic generator).

## [Unreleased] — Batch 2 tool-rag

### Added

- Tool-RAG routing foundation: `crates/tool_rag` (SQLite index `rag_tools`/`rag_terms`/`rag_receipts`, deterministic tokenizer, `LocalLexicalProvider` + `DisabledEmbeddingProvider`, baseline ranking, 19 tests).
- Ranking: filters first (disabled never rank, capability allow-list, risk ceiling `low<medium<high<irreversible`, evidence requirement), then keyword + capability bonus + success boost (capped) + evidence boost − failure/denial penalty (capped, floor 0); ties break by tool id; every candidate self-explains.
- Index sources: manifests + tool-receipt outcomes + task titles + evidence kinds + compensation strategies (for tools that require them), folded deterministically per tool.
- Desktop IPC: `queryToolCandidates` (read-only; 21 commands total); UI Tool-RAG panel on the Tools page (intent box, capability/risk/evidence/top-K filters, ranked candidates with score/reason/risk badge/approvals, manifest links).
- Contracts `toolrag.ts`: `ToolCandidate`, `ToolRagQuery`; `ToolId` unchanged.
- ADR `docs/adr/0004-tool-rag-foundation.md`: normalized tables over FTS5, lexical baseline, disabled-by-default embeddings.

## [Unreleased] — Batch 2 context

### Added

- Context budgeting + TypeCompact foundation: `crates/context_budget` (7-layer model, local char-fallback estimator, priority-ordered rebalancing, deterministic TypeCompact, structured invariant gate, deterministic snapshot builders, 20 tests).
- Budget rules: 8_000 total tokens (safety 800, system 800, task 1600, session 1200, evidence 1600, tool 1200, scratchpad 800); protected layers (safety/system/task/session) verbatim; evidence trims but keeps refs; tool/scratchpad absorb overflow first.
- TypeCompact: preserves IDs, safety constraints, policy decisions, evidence/compensation refs, schema type names; compresses repetitive text (dedupe + explicit trim marker); rejects any invariant loss with structured `invariant_*` errors; every result carries original/compacted estimates, preserved invariants, dropped/trimmed lists, and a `sha256:` proof hash.
- Desktop IPC: `contextBudgetStatus`, `contextCompact` (compact + persist verified `context_compaction` evidence + emit `contextCompacted`), `contextDiagnosticSnapshot` (20 commands total); task detail reads the budget; diagnostic flow produces a compacted snapshot.
- Contracts `context.ts`: `ContextLayer` (7), `ContextBudgetStatus`, `ContextCompactedSegment`, `ContextCompactionReceipt`; `ToolId` unchanged.
- UI Context page (`/context`): budget bars per layer, compacted preview, invariant checklist, compaction receipt panel, protected items list; nav + command palette + feed entries.
- ADR `docs/adr/0003-context-budget-typecompact.md`: Rust-first deterministic engine, no external calls, invariant-gated persistence.

## [Unreleased] — Batch 2 computer-use

### Added

- Computer-use safety layer: `crates/computer_use` (safety envelope, deterministic mock virtual screen, disabled real-adapter stub with `real-os-control` compile-error gate).
- 9 policy-gated tools (`computer.move_mouse/click/double_click/drag/type_text/hotkey/scroll/capture_screen/locate_element`) in `tool_runtime` with strict validation, envelope gating, dry-run plans, mock execution, and evidence receipts (screenHash, boundingBox, confidence).
- Policy: `UiAction`-Low allow rule (read-only), `UiAction`-Medium evidence gate (move/scroll), High non-reversible → human review; high-risk escalates, never downgrades.
- Desktop IPC: `computerSafetyStatus`, `computerEmergencyStop` (latches stop, cancels active queue tasks, emits `computerSafetyChanged`), `computerResetStop` (17 commands total); envelope held in shared `AppState`.
- UI Computer page (`/computer`): safety status panel, emergency stop/reset controls, mock screen visualization with bounding-box overlay, action builder with high-risk flags, dry-run/live buttons, policy panel, receipt viewer; status-bar pill (STOPPED/armed/disabled).
- Safety rules: global enable flag off by default; dry-run default; 30/min rate limit on live actions; 1440x900 viewport bounds; 0.7 confidence floor; system-settings + password-manager hard blocks; terminal + payment-flow warnings.
- ADR `docs/adr/0002-computer-use-safety-layer.md`: Rust-first authority, mock-only, no live path yet, emergency-stop semantics.

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

- GitHub repository `shubhanand021-EXA/talos` (private at creation; public since HF-03 per ADR 0005) with full Batch 1 history and `v0.1.0-green-baseline` tag.
- Enterprise storefront: premium README with badges, hero GIF, feature grid, quickstart, architecture, Target Capability Matrix, roadmap, security, contributing, license, acknowledgements.
- Brand/media system: `scripts/media/generate-brand.mjs` (`pnpm media:generate`), original `talos-logo.svg`, `talos-banner.svg`, `hero.gif`, `evidence.gif`, `architecture.svg` under `docs/media/` (deterministic, dependency-free, offline).
- GitHub community health: `CONTRIBUTING.md`, `SECURITY.md`, `CODE_OF_CONDUCT.md` (Contributor Covenant 2.1), `SUPPORT.md`, `.github/CODEOWNERS`, `.github/FUNDING.yml`, issue forms (`bug_report`, `feature_request`, `security_report`, `question`, `config`), PR template with audit/risk/rollback sections.
- Hardened CI (`.github/workflows/ci.yml`): push/PR/dispatch triggers, pnpm + cargo caching, `setup/meta`, `typescript`, `rust-core`, `desktop-check`, `audit`, `media-check` jobs, artifact upload, honest isolation of platform-dependent steps; documented in `.github/workflows/README.md` and `docs/CI.md`.
- Docs refresh: `docs/ARCHITECTURE.md`, `docs/PROGRESS.md`, `docs/EVIDENCE.md`, `docs/ROADMAP.md`, new `docs/CI.md`, `docs/media/README.md`.

### Changed

- Default branch is `main` (renamed from `master`); remote `origin` tracks `https://github.com/shubhanand021-EXA/talos.git`.
- Repository metadata: public visibility (private at creation; public since HF-03 per ADR 0005), description, topics (`ai`, `agents`, `ai-agents`, `desktop-app`, `tauri`, `rust`, `react`, `typescript`, `orchestration`, `browser-automation`), Issues enabled, Projects/Wiki disabled.
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
