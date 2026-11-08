# Talos Architecture — Pentagon Architecture

> Repo: `https://github.com/shubhanand021-EXA/talos` (public, default branch `main`; see [ADR 0005](adr/0005-public-repository-transition.md)). Batch 2 storefront: enterprise README, brand/media system (`docs/media/`), hardened 6-job CI ([docs/CI.md](CI.md)). Control-loop diagram: [docs/media/architecture.svg](media/architecture.svg).

Status: **v0.4.0 — all five Pentagon systems live**: Foundation (Tauri + Rust + React), Brain (Tool-RAG, context budgeting, live browser, computer-use, LLM gateway, assembly, memory), Soul (evidence ledger + compensation + persistence), Face (Void Neon UI), Shield (sandbox, encryption, signed updater, audit swarm). What was "specified but not implemented" in Batch 1 is now implemented; the per-section status lines below are authoritative.

## 1. Foundation: Tauri 2 + Rust + React

- Desktop shell in `apps/desktop/`
- UI package in `apps/ui/`
- Rust workspace in `crates/`
- Shared TS contracts in `packages/contracts/`
- pnpm workspaces + strict TypeScript + `just` task runner
- Reproducible setup via `just setup` and `just doctor`

Batch 1 scope (all delivered, v0.1.0): monorepo skeleton, tooling, hygiene, CI, shared TS contracts,
Rust workspace with Policy Broker core + SQLite evidence store + agent runtime + hash-chained ledger and compensation plans,
Tauri desktop shell with typed IPC and live local loop.
Batches 2–4 delivered the rest on top: file-backed persistence (Batch 4),
real agents and tools (Batches 2–3), full audit swarm (Batch 2).

## Desktop shell (`apps/desktop/`)

- Tauri 2 app `com.talos.desktop`, window `Talos` (1440×900, min 1280×800)
- `frontendDist: ../ui/dist` (production), `devUrl: http://localhost:5173` + `beforeDevCommand: pnpm --filter ui dev`
- `src-tauri` is a standalone Cargo workspace with path deps into `crates/`
- `AppState`: `DefaultPolicyEngine` + encrypted-at-rest SQLite `EvidenceDb` (ChaCha20-Poly1305 envelope, keychain-held key, WAL, migrations `0001-0012` + `schema_version`, dirty-shutdown flag, checkpoint resume) + `LocalOrchestrator` (Mutex-guarded) + MCP server/plugin registry + staged-update slot, managed by Tauri
- **Batch 5 daemon/client split**: the Tauri shell is now a _client_ of the persistent `talos-gateway` daemon (`crates/talos_gateway`, see §2m). On startup it probes `ws://127.0.0.1:8080/talos`, spawns a detached daemon child when absent, routes IPC through it (`gateway_forward`), and heartbeats it (`gateway_status` / `gateway_ping` / `gateway_restart`). The daemon holds the exclusive WAL lock; closing this window never stops it. All 100 commands remain available locally during the transition.
- One hundred typed IPC commands (tasks, tools, MCP/plugins, skills/GEPA, sandbox, computer, context, Tool-RAG, providers, assembly, delegation, planning, workspaces, memory, approvals, telemetry, storage, onboarding state, updater channels/staging/verification, browser profiles/HITL); structured `{ code, message }` errors
- `run_diagnostic` runs the echo-agent loop and emits one event per stage; `submit_task` enqueues durable work and executes it through the policy broker; 5s `heartbeat`
- UI bridge (`apps/ui/src/bridge/`): typed `invoke` + Zod validation, `useTalosSync` subscription hook, stores hold live agents/feed/evaluations
- Capability `default`: main window only, zero extra permissions; strict CSP
- Event names string-identical to `@talos/contracts` `IpcEventName`
- Root scripts: `pnpm dev:desktop`, `pnpm build:desktop` (`just dev-desktop` / `just build-desktop`)

## Rust workspace layout (`crates/`)

```text
crates/
  Cargo.toml          # workspace: talos_common, policy_broker, evidence_store, agent_runtime, tool_runtime, computer_use, context_budget, tool_rag, sandbox, talos_secrets, llm_gateway, context_assembler, workspace_manager, embedding_engine, vector_store, memory_indexer, talos_telemetry, cost_tracker, batch2_demo, awakening_demo, mcp_client, mcp_server, instruction_parser, skill_extractor, talos_cli, talos_updates, talos_gateway, scheduler, channels, remote_runner
  talos_common/       # TalosError/Result, typed IDs, time + serde + tracing helpers
  policy_broker/      # PolicyEngine trait + DefaultPolicyEngine + context hooks, talos-policy-broker --selftest
  evidence_store/     # encrypted-at-rest SQLite store (envelope, WAL, schema_version, backup/restore, dirty-flag + checkpoint crash recovery), migrations 0001–0012, append-only repo + shared app_state KV
  agent_runtime/      # lifecycle state machine + durable queue + worker/supervisor/delegation/planner + MockEchoAgent scenarios
  tool_runtime/       # ToolId/Manifest/Invocation/Receipt + Registry + adapters + fs/exec/code/git/browser/computer tools + gated executor
  sandbox/            # OS process sandbox: profiles, bubblewrap/Seatbelt/Job-Object enforcers, fail-closed receipts
  talos_secrets/      # OS-keychain vault + hermetic mock backend
  llm_gateway/        # BYOK providers (OpenAI/Anthropic/Ollama) + router + budget tracker
  context_assembler/  # gather + assemble + Tool-RAG catalogs (TypeCompact via context_budget) + workspace instruction files (AGENTS.md/CLAUDE.md/SKILL.md, verbatim)
  mcp_client/         # MCP client: JSON-RPC 2.0 over stdio/SSE, policy-gated tools/call, 30s timeout with process kill
  mcp_server/         # Talos as MCP server: bearer-token tools/list + policy-authorized tools/call over the 33 built-ins
  instruction_parser/ # AGENTS.md/CLAUDE.md/SKILL.md/HEARTBEAT.md parsers + plugin manifests (ed25519) + permission reviews
  workspace_manager/  # per-task isolated roots: git worktrees or temp dirs + worktree-aware cleanup
  computer_use/       # safety envelope + virtual-screen mock + feature-gated real OS driver
  context_budget/     # layered budgets + deterministic TypeCompact + invariant gate (no external calls)
  tool_rag/           # local-first tool retrieval: SQLite index + lexical/semantic ranking
  embedding_engine/   # deterministic offline embeddings (hash-384 + TF-IDF, no native deps)
  vector_store/       # SQLite vector tables + brute-force cosine search in the shared EvidenceDb
  memory_indexer/     # incremental ledger indexing, hybrid recall, skill distillation
  skill_extractor/    # GEPA loop: verified-task extraction, watermarked evaluation, published-skill retrieval
  talos_cli/          # headless `talos` binary: run/task/evidence/audit/config/provider/mcp/skill/version/doctor over shared crates, JSONL + exit codes 0/1/2
  talos_gateway/      # persistent headless daemon: WAL owner, ws://127.0.0.1:8080/talos, Command/Event protocol (Batch 5)
  scheduler/          # HEARTBEAT.md natural-language scheduler: triggers, cron mapping, tick engine (Batch 5)
  channels/           # external messaging surfaces: Telegram/Discord adapters behind the policy broker (Batch 5)
  remote_runner/      # BYOC execution targets: SSH/Docker runners with local policy gating (Batch 5)
  talos_updates/      # release channels, signed-manifest validation (minisign-verify), staged-rollout gating
  talos_telemetry/    # local-first spans, no external endpoints
  cost_tracker/       # real pricing/budgets, reservations, child-budget settlement
  batch2_demo/        # deterministic Batch 2 E2E proof (headless)
  awakening_demo/     # deterministic Batch 3 E2E proof (headless + live opt-ins)
```

- Workspace lints forbid `unsafe_code`; binaries commit `Cargo.lock`.
- Policy decisions mirror `@talos/contracts`: allow / deny / require_evidence /
  require_compensation / require_human_review, default deny.
- Quality gates: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`, `talos-policy-broker --selftest`.

## 2. Brain: Tool-RAG + context budgeting + TypeCompact

- Tool registry with retrieval (Tool-RAG) — **live**: `crates/tool_runtime` exposes manifests + JSON schemas; `crates/tool_rag` indexes manifests + receipts + evidence + task/compensation context in SQLite (normalized tables, deterministic tokens) and ranks with lexical + semantic providers over local embeddings (hybrid blending with receipt-history boosts; Batch 3 §2e-i)
- Explicit context budgets per turn — **live**: `crates/context_budget` accounts seven layers (safety/system/task/session/evidence/tool/scratchpad) against an 8_000-token budget with a local char-fallback estimator
- TypeCompact: schema-first compaction for long histories — **live**: deterministic TypeCompact preserves IDs, safety constraints, policy decisions, evidence/compensation refs, and schema type names; drops only reference-free segments on discardable layers; every result carries a `sha256:` proof hash and is invariant-gated before persistence

Status: **tool + browser registries, context budgeting/compaction, and Tool-RAG baseline live locally (Batch 2 tools/browser/computer/context/rag); browser live Chromium production-ready behind an explicit opt-in (Batch 3); semantic/hybrid ranking over local embeddings live (Batch 3, §2e-i)**. Target: Batch 5+ advanced retrieval.

## 2b. Tools: policy-gated execution framework (Batch 2, live)

- `crates/tool_runtime`: `ToolId` / `ToolManifest` (risk, capabilities, reversibility, evidence/compensation needs, JSON schemas) / `ToolInvocation` / `ToolReceipt` (status, policy verdict, evidence/compensation links) + `ToolRegistry` (register/list/get/validate, metadata for UI + RAG)
- Safe built-ins (33): `fs.read` / `fs.list` (bounded, no symlink escape) / `fs.search` (bounded) / `fs.write` (policy + evidence + `backup_restore` compensation before mutation) / `fs.edit` (unified-diff apply, dry-run validated, backup + compensation) / `fs.create` / `fs.delete` (irreversible, backup + compensation) / `fs.move` / `process.exec` (disabled by default, per-invocation opt-in for workspace coding tasks, coder allowlist, denylist, 60s default / 300s max timeout, minimal env, 100KB hashed capture) / `code.run_tests` (auto-detected cargo/pnpm/npm/pytest, bounded hashed output) / `code.install_deps` (manifest-aware, network-gated) / `git.status|diff|branch` (read-only Low, workspace-scoped) / `git.commit` (Medium + evidence, message validated) / 9 `browser.*` tools (isolated profiles, DOM-first safety, mock deterministic, production live Chromium behind `TALOS_BROWSER_LIVE=1`) / 9 `computer.*` tools (virtual screen, safety envelope, mock-only)
- Coding workspaces (`crates/workspace_manager`, Batch 3): per-task isolated roots — git worktree (`-b talos/<task>`) when a repo is hinted, else temp dir; workspace root is the only filesystem root for the task's tools; cleanup removes the worktree registration + dir; file/shell/git/code tools resolve under it with symlink-escape rejection intact
- Adapters: `DryRun` (default for risky tools, no side effects) / confined local execution / OS sandbox backends (bubblewrap/Seatbelt/Job Objects, fail-closed — see §2b-iii) / BYOC remote runners (SSH/Docker — see §2b-iv)
- Policy: every invocation becomes an `ActionRequest` (capability + risk from manifest); write/exec need evidence; browser mutations need evidence (dry-run first); computer reads run directly while mutations need evidence and High-risk actions need human review; high-risk flags escalate, never downgrade; irreversible-equivalent needs compensation; denials produce structured codes (`policy_denied`, `tool_disabled`, plus safety codes). **Remote evaluation happens locally before anything is sent**: denied remote calls never touch the wire (proven by mock-transport tests).
- Evidence: every receipt persisted as `tool_receipt` evidence (verified), including denials; transitions journaled
- Desktop: `list_tools`, `describe_tool`, `invoke_tool_dry_run`, `invoke_tool`, `computer_safety_status`, `computer_emergency_stop`, `computer_reset_stop` (17 commands total, generic across tools); UI Tools page (`/tools`) + Browser page (`/browser`) + Computer page (`/computer`) with catalogs, dry-run forms, receipt viewers
- Architecture decisions: [docs/adr/0001-browser-use-architecture.md](adr/0001-browser-use-architecture.md) (Rust authoritative, TS worker + opt-in live adapter), [docs/adr/0002-computer-use-safety-layer.md](adr/0002-computer-use-safety-layer.md) (Rust-first, mock-only, disabled by default, emergency stop)

## 2b-iii. OS sandbox: namespaces, Seatbelt, Job Objects (Batch 4, live)

- `crates/sandbox`: `SandboxProfile` (`read_only` = no execution / `workspace_write` = confined / `danger_full_access` = full access by explicit approval) / `SandboxConfig` (profile, workspace root, network flag + host allowlist, cpu/memory caps, timeout, allowed commands, danger approval id) / `SandboxReceipt` (`profile`, `enforced`, `enforcementMethod`, `networkPolicy`, `violations`) + `SandboxEnforcer` trait (platform dispatch, `wrap_command`, post-spawn `PostSpawnGuard`) with fail-closed `enforce_or_deny` (`sandbox_unavailable` — never unsandboxed)
- Linux bubblewrap: ro toolchain binds (`/usr /bin /lib /lib64 /etc`, Cargo/Rustup homes), rw workspace bind (canonicalized, tmpfs-before-bind ordering), `--unshare-net` default, `prlimit` RLIMIT_CPU (timeout-derived) / RLIMIT_AS / RLIMIT_NPROC=128, best-effort cgroup v2 clamp (`cpu.max memory.max pids.max`, `TALOS_CGROUP_ROOT` test hook); missing `bwrap` denies (CI installs it for escape tests)
- macOS Seatbelt: per-config `.sb` generation (writes only under workspace, network deny/allowlist) enforced via `sandbox-exec`; resource limits are timeout-kill only (documented gap); unavailable off-macOS denies
- Windows Job Objects (via `windows` crate, four documented `unsafe` blocks — the only `unsafe` in the workspace): ActiveProcessLimit anti-fork-bomb, per-process/job memory caps, CPU-rate hard cap; assignment post-spawn with the guard held to reap; no filesystem namespace (containers required — documented); no per-process egress control (policy layer)
- `tool_runtime` integration: `process.exec` / `code.run_tests` / `code.install_deps` gate before spawning (dry-run untouched); manifests carry `sandbox_profile: workspace_write`; receipts embed the `SandboxReceipt` and persist as evidence; denials are `Denied/sandbox_unavailable`
- Desktop: `get_sandbox_status`, `get_sandbox_profile_for_task` + `sandboxEnforced` / `sandboxViolation` events; danger profile holds for HITL approval (`approval_required`, grant then retry); UI Computer-page OS sandbox panel (platform, backends, warning banner, per-task lookup); architecture decision: [docs/adr/0010-os-sandbox-architecture.md](adr/0010-os-sandbox-architecture.md)

## 2b-iv. BYOC remote runners: SSH/Docker execution targets (Batch 5, live)

- `crates/remote_runner` (new): `.talos/runners.json` registry (`id`, `ssh`|`docker` kind, host/user/keyPath/remoteDir or image/context/workdir/cpus/memory/network, `enabled`) — secret-free (key _paths_ only, never key material). `CommandRunner` trait (argv-exact local spawn, kill-on-timeout + reap, bounded capture) with `SystemRunner` (env-cleared + `SystemRoot`/`LANG` only) and scripted `MockRunner` for hermetic tests. SSH: `BatchMode` + `ConnectTimeout` + no agent/X11 forwarding, workspace sync over a `tar` pipe (no rsync, no local shell), commands run via a quoted `sh -c` under the remote dir, results copied back capped (`remote_sync_truncated` instead of silent drops). Docker: `run --rm --network none -v <stage>:/work -w <workdir> <image> <cmd…>` — exactly one bind-mount (the staged workspace), ephemeral container, caller-chosen CPU/memory caps.
- `crates/tool_runtime`: `ExecutionTarget::{Local, Remote { runner_id }}` on `SandboxConfig` (default `Local`; per-task assignment via `set_task_runner`). Executor routes **only** exec-family tools (`process.exec`, `code.run_tests`, `code.install_deps`) remotely — everything else denies `remote_unsupported` (fail closed, never silently local); unlinked runners deny `remote_unlinked`; allowlist + denylist re-apply locally before anything is sent; timeouts kill + reap with structured `timeout` / `remote_unreachable` / `remote_failed` codes.
- Evidence & fidelity: receipts carry `remoteRunnerId` + `outputSha256` (hash of bounded remote output) and persist to the **local** ledger like any receipt; fidelity is explicitly `live_remote` (new `ExecutionFidelity` rank between `live` and `verified`; local work can never claim it — `check_fidelity_claim` fails, both Rust + TS). Unproven attempts (transport/timeout before any remote process ran) stay `mock`.
- Desktop: `runners.json` under app-data (`list/add/remove/test/set_task_runner/get_task_runner` IPC; `test_runner` runs `ssh … true` / `docker info` with a 15s cap). UI Settings → **Runners** tab (link/unlink with key paths, test connection, per-task assignment). Contracts: `ExecutionTargetType`, `RemoteRunnerConfig`, `RunnerStatusView` (`packages/contracts/src/remote.ts`).
- Security boundary: policy is evaluated **locally** first (broker verdict precedes any byte sent remote — a `Deny` never opens a socket, covered by mock-transport tests); remote side sees only validated argv under an unprivileged user inside one directory (SSH) or one bind-mount in an air-gapped container (Docker); no host root, no credentials, no shell init cross either boundary.

## 2b-i. Live browser worker (Batch 3, production)

- `packages/browser-use` (Playwright 1.64.1, optional peer): `LiveBrowserAdapter` executes all 8 actions against real Chromium — `launchPersistentContext` per task profile (headless default, `--no-first-run --disable-default-apps --disable-extensions`, downloads rejected), selector resolution + visibility/interactable checks before every mutation, `domcontentloaded` navigations, masked password regions, content-addressed screenshot artifacts (`<5 MiB`, skipped on sensitive pages), bounded JPEG thumbnails, NDJSON/SSE-free direct CDP-less Playwright calls, structured timeout/not-found/not-visible/navigation errors without page-content leakage.
- Safety gates (worker + Rust, neither bypassable): live requires `TALOS_BROWSER_LIVE=1` (else mock/disabled); mutations default to dry-run; high-risk needs explicit `approved: true` (`approval_required` denial + `browserAuthRequired` event otherwise); password fields never auto-filled (`auth_required`); navigation restricted to `http(s)` with scheme blocklist (`file/chrome/about/javascript/data`) and optional origin allowlist (`TALOS_BROWSER_ALLOWED_ORIGINS`); sensitive domains (`TALOS_BROWSER_SENSITIVE_DOMAINS`) suppress artifact storage.
- Sessions: `LiveSessionManager` (max 1 warm by default, 5-minute idle auto-close, `session.launch|status|close|close_all` over JSON-RPC `--serve` or single-shot argv/stdout); `browser.session` tool exposes it through the generic IPC; emergency-stop path closes all warm sessions.
- Rust: `tool_runtime::browser_worker` spawns `node worker.js` per live call (30s kill timeout, stdout-parsed receipt, `browser_worker_crashed/timeout/protocol` structured errors); executor routes live browser arms to the worker only when opted in, mock otherwise; crashes emit `browserWorkerCrashed` and mark the task failed with ledgered evidence. Raw keys, prompts, and page content never cross into evidence — only hashes, metadata, and bounded extracts.

## 2b-ii. HITL approvals: Plan/Act + diff viewer (Batch 3, live)

- `agent_runtime::approval`: `ApprovalStore` (pending queue, per-task Plan/Act/Auto modes defaulting to Plan, scoped "don't ask again" opt-outs, `PlanDocument`, `CompensationExecution`); every `require_human_review` verdict opens a full `ApprovalRequest` (summary, exact target, effect class `READ…PRIVILEGED`, reversibility, payload/diff, risk, required checks, compensation, cost, scope, 5-minute expiry)
- Lifecycle: task gates in `waiting_policy`; grant → `executing`, deny → `failed`, failed compensation → terminal `failed_needs_manual_review` (retryable to `queued`)
- Desktop: 11 IPC commands (`get_pending_approvals`, `request_approval_review`, `grant/deny/modify_approval`, `batch_approve_low_risk`, `set/get_task_mode`, `get_plan_document`, `execute_compensation`, `fail_compensation_step`) + 5 events (`approvalRequested/Granted/Denied/Expired`, `compensationExecuted`); contracts `approval.ts` is the single source of truth
- UI `features/approval`: non-dismissable ApprovalSheet (9 sections, Enter/Escape/M, focus trap, a11y labels, reduced-motion safe), DiffViewer (word-level diff, collapsible regions, multi-file tabs), PlanView, CompensationSheet, queue panel; Shell overlay, amber status-bar pulse, Void Neon gate click-to-review, Dashboard/Agents surfaces, command-palette entry

## 2c. Computer-use: safety-first foundation (Batch 2, live mock)

- `crates/computer_use`: `ComputerAction` (9) / `ComputerTarget` (coords, bounds, app/window hints, confidence) / `ComputerReceipt` (screenHash, boundingBox, confidence) / `SafetyEnvelope` (global enable flag off, dry-run default, 30/min rate limit, 1440x900 bounds, 0.7 confidence floor, dangerous zones, e-stop latch) / `MockComputer` (virtual screen, seeded elements, deterministic hashes) / `RealComputerAdapter` trait + disabled stub + `real-os-control` compile-error gate
- Safety envelope runs before every adapter: stop latch blocks everything (even dry-run); disabled flag blocks live; rate limit caps live actions; bounds/zones/confidence gate both modes; warnings surface in receipts
- Policy: read-only computer tools (Low + UiAction) run directly; move/scroll (Medium) need evidence first; mutations (High, non-reversible) require human review; highRisk flag escalates, never downgrades; denials carry structured codes (`emergency_stop`, `computer_disabled`, `rate_limited`, `out_of_bounds`, `dangerous_zone`, `low_confidence`)
- Desktop: `computer_safety_status`, `computer_emergency_stop` (latches + cancels active queue tasks + emits `computerSafetyChanged`), `computer_reset_stop` (20 commands total with context); UI Computer page (`/computer`) with safety panel, mock screen visualization, action builder, dry-run/live buttons, receipt viewer, e-stop controls; status bar shows STOPPED/armed/disabled

## 2c-i. Real OS driver: accessibility + vision loop (Batch 3, feature-gated)

- `computer_use` behind `real-os-control` (never CI/default): `AccessibilityDriver` trait (Windows UI Automation, macOS AXUIElement, Linux AT-SPI2, coordinate fallback), `enigo` input with kill switch, `xcap` screenshots + `sha256:` verification, session isolation (`SessionScope` + budgets in the envelope; payment/banking now hard-block per ADR 0008), injection guard with e-stop accounting, content-addressed artifacts + thumbnails
- Observe → act → verify per action; focus loss and manual input fail closed; `actionVerificationFailed` / `computerFocusLost` events; real e-stop log line with in-flight count
- Desktop: `computer_session_scope`, `computer_set_session_scope`; UI live view (per-action thumbnails, bbox overlay, timeline, Take Control, allowlist editor, confidence). See ADR 0008 + `docs/COMPUTER_USE_REAL.md`

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
- Hybrid retrieval (Batch 3, memory): `RetrievalMode::{Lexical,Semantic,Hybrid}` + `rank_with_semantic` (max-normalized blend, empty map ≡ legacy rank); `query_tool_candidates` accepts an optional `mode` (lexical default; semantic/hybrid embed intent + tool texts with local embeddings and blend, alpha 0.5/1.0)

## 2e-i. Semantic memory: local embeddings + vector recall (Batch 3, live)

- `crates/embedding_engine`: deterministic offline embeddings — 384-dim hashing-trick backend (`local-hash-384/1`, L2-normalized) + corpus-fitted TF-IDF vectorizer; `cosine_similarity`; `.talos/models/` probe reporting the hash fallback (no ONNX runtime bundled; see [docs/adr/0007-semantic-memory-local-embeddings.md](adr/0007-semantic-memory-local-embeddings.md)).
- `crates/vector_store`: SQLite vector tables in the shared `EvidenceDb` (`vector_entries` + `memory_index_state`); content-hash dedupe (unchanged rows skip re-embedding); filtered brute-force cosine search (source type, task, date range, verified-only); <100ms top-10 at 1k rows, zero native deps.
- `crates/evidence_store`: migration `0006_skills.sql` (`skills` quarantined→validated→published, no skips); `create/get/list/update_skill_status`; migration `0011_skill_stats.sql` (GEPA: `usage/success/failure_count`, `confidence_score`, `last_used_at`, `parent_version`; lifecycle extended to `published→deprecated`, `any→archived`; `record_skill_use`, `create_skill_version` with rollback, bounded LCS `skill_diff`, stale sweeps).
- `crates/memory_indexer`: incremental indexing (claims, verified-first evidence, completed tasks, skills; proposes quarantined skills from completed tasks with verified claims, deduped by source task) + `rebuild_index` + hybrid `hybrid_search` (scope filter → lexical + semantic → normalized blend → recency/verified rerank → content-hash dedupe → top-K with source refs) + 60s background indexer in `AppState`.
- `crates/skill_extractor` (GEPA learning loop, Batch 4): deterministic no-LLM extraction from verified completions (`tool_receipt` chain + recorded params → SKILL.md validated by `instruction_parser`, born quarantined + indexed; `agent_runtime` hook best-effort on verified completion); watermarked evaluation every 15 receipts (recurring sequences reinforce skills or propose one; sustained failure / 30-day staleness auto-deprecates; sub-threshold flags review; background-spawned, never blocking); `gather_published_skills` (published-only + ceiling-covering structured procedures) and `published_skill_tools` for Tool-RAG.
- `crates/tool_rag`: `rank_with_skill_boost` (`+0.2` published-skill membership, empty set ≡ baseline); `context_assembler::gather_skill_segments` (protected tool-layer procedures) + `rank_tools_with_skills` feed `preview_context` (skill segments ride the tool block verbatim; quarantined never qualifies).
- Context assembly: `semantic_segments` maps hybrid hits into session-layer segments competing in the existing layer budget; `preview_context` gathers top-5 hybrid recall and skips already-gathered evidence.
- Desktop: `memory.rs` IPC (`search_memory` indexes incrementally first, `get_index_status`, `rebuild_index`, `list_skills` with usage stats + confidence) plus `skills.rs` IPC (`get/validate/publish/deprecate/archive_skill`, `get_skill_stats`, `list_skill_versions`, `diff_skill_versions`, `trigger_skill_evaluation` spawning the GEPA background pass; `invoke_tool` auto-spawns when the 15-receipt cadence is due; `submit_task` emits `skillCreated` for newly distilled skills); events `memoryIndexed`, `skillCreated/Validated/Published/Deprecated/Archived/EvaluationCompleted`, `indexRebuilt`.
- UI: `/memory` route + nav + palette; Memory page (index status + rebuild, hybrid search with source/mode/verified filters, scored result cards, 5-status skill registry with usage stats) + Skill Manager (lifecycle buttons, full SKILL.md viewer, version history + diff, semantic skill search, Evaluate Now); Memory link on the Evidence page; `useTalosSync` memory events.
- Architecture decision: [docs/adr/0012-gepa-skill-learning.md](adr/0012-gepa-skill-learning.md) (deterministic extraction, watermarked evaluation, human-gated promotion).

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

## 2h. Persistence layer: encrypted, recoverable, durable (Batch 4, live)

- `evidence_store::storage`: app-data resolution (Tauri `app_data_dir` for
  `com.talos.desktop`, `TALOS_DATA_DIR` override for tests/portable
  installs) → `<app_data>/talos.db` + `<app_data>/backups/` +
  `<app_data>/models/`; the resolved path is logged at startup (path only).
- At-rest encryption (`crypto.rs`, Batch 4): ChaCha20-Poly1305 file envelope
  (`TALOSENV` magic + version + random 12-byte nonce per encryption, AEAD
  over the header) under a 256-bit key held in the OS keychain
  (`com.talos.desktop` / `database.encryption_key`; first run generates +
  stores, later runs retrieve; keychain-unavailable falls back to plaintext
  with a loud warning + `encryption_unavailable` event, never silently).
  SQLite operates on a decrypted working copy (`talos.work.db`, `0600` on
  Unix, wiped on clean shutdown); the canonical `talos.db` on disk is always
  ciphertext while encrypted. Existing plaintext databases are checkpointed
  then encrypted in place on first encrypted startup (migration); unknown
  files are refused, never deleted. (`sqlx` 0.8 has no SQLCipher feature, so
  page-level encryption is unavailable — the envelope is the documented
  equivalent. See [SECURITY.md](SECURITY.md) for the honest limitations:
  running-process reads, best-effort wipe, loud plaintext fallback.)
- `EvidenceDb::connect_encrypted_file` / `connect_file`: create parents, WAL
  - FK + 5s busy timeout + `NORMAL` synchronous, idempotent migrations
    `0001–0011` mirrored into `schema_version`; `connect_memory` remains for
    tests only. Pool drain waits for sqlx's async finalizer before file
    renames/deletes (Windows file-locking discipline).
- Read-then-write chain appends use `BEGIN IMMEDIATE` (no SHARED → RESERVED
  deadlock under concurrency); post-restore verification re-runs
  `verify_chain`.
- Backup (`VACUUM INTO talos-<timestamp>.db`, newest-5 retention, sealed with
  the same envelope so backups at rest are encrypted too) and transactional
  restore (validate → single-transaction content copy → verify; encrypted
  backups decrypt to a wiped-after-use temp; a foreign sealed backup without
  the key is refused).
- Crash recovery (`recovery.rs` + `checkpoints.rs`, migration `0010`, Batch 4):
  a `shutdown_state` flag is set dirty on every startup and cleared only on
  orderly shutdown, so a dirty flag on boot proves the previous process died
  mid-flight. `run_startup_recovery` on every production connect: chain
  verdict (`startup_integrity_verified` / `_broken`, never a startup
  refusal) plus per-task reconciliation — `executing` + checkpoint →
  `queued/resumed_from_checkpoint` (with a `task_resumed` ledger event naming
  the checkpoint), `executing` without checkpoint →
  `failed/crash_recovery_failed`, `waiting_policy` →
  `failed/approval_lost_on_restart`, `waiting_*` → `failed`, `canceling` →
  `canceled`, `planning` → `queued`, `queued` untouched. Approval loss is by
  design: the `ApprovalStore` is in-memory, so interrupted approvals fail
  their tasks instead of silently resuming. Checkpoints (task status, chain
  head, bounded workspace digest, tool-receipt count) are ledgered as
  `task_checkpoint` evidence every 5 tool calls by default
  (`TALOS_CHECKPOINT_EVERY` overrides, `0` disables; best-effort hook in the
  `tool_runtime` executor — checkpointing never fails the tool call). Orphan
  working files are adopted (latest state wins); stale non-database files are
  quarantined, never deleted.
- Desktop (`state.rs` encrypted open + shutdown seal, `storage.rs`, 4 IPC
  commands + 3 events): `get_storage_info` (now reports `encryptionEnabled`
  - `encryptionMethod`), `create_backup`, `restore_backup`
    (path-validated), `list_backups`; `backupCreated`, `backupRestored`,
    `integrityCheckCompleted`. UI: Storage section in Settings (path, sizes,
    schema, integrity, encryption badge, backup list, Create + confirm-gated
    Restore) + storage health pill in the status bar; contracts `storage.ts`
    is the single source of truth.

## 2i. Supervised delegation with enforced budgets (Batch 4, live)

- `agent_runtime::supervisor`: `SupervisorConfig` (fan-out 1..=16, depth 1..=4;
  defaults 4/2) is enforced per spawn — `validate_spawn` reads the
  supervisor's own config, never module constants. Runtime retuning via
  desktop `update_supervisor_config` (validated 1..=16 / 1..=4,
  evidence-logged as `supervisor.config_updated`).
- `agent_runtime::planner` (LLM-driven DAG planning, `PLANNING_PROMPT_VERSION
= "1.0"`): `PlanningPromptTemplate` renders a deterministic prompt (goal,
  roles/capabilities, available tools, budget, max depth/fan-out, strict
  `{"plan_version","steps","total_estimated_tokens","reasoning"}` schema).
  `SupervisorAgent::plan_with_llm` calls the model through a
  `PlanningLlmClient`, parses strict JSON, retries once with a correction
  prompt, and falls back to keyword decomposition (labeled
  `PlanningMethod::Keyword`) when no key is configured, the call times out
  (>30s), JSON fails twice, or validation rejects. Request/response hashes
  plus token estimates are recorded as evidence; planning cost flows through
  the gateway's usage records.
- `validate_plan` deterministic gates (LLM output is untrusted): schema
  (`plan_version`, roles, goals, token ranges), dependencies (exist + no
  cycles via topological sort), depth (longest chain ≤ `max_depth`),
  fan-out (widest level ≤ `max_fanout`), tools (every `tools_needed` exists
  in the registry AND inside its role's allowlist), budget
  (`total_estimated_tokens` equals the step sum and fits the token budget),
  risk (`high` → `requires_approval`, held for HITL before any spawn).
- DAG execution: levels run in topological order (sequential within a
  level, bounded by validated fan-out); a failed dependency cancels all
  dependents (`canceled`, no tools executed). Every step's tools still pass
  the policy broker at execution time — planning can never widen authority.
  Recorded as a `plan_artifact` evidence row + `planning.completed` ledger
  event; `planGenerated` / `planValidated` / `planRejected` / `planFallback`
  IPC events drive the UI.
- Desktop IPC: `preview_plan` (DAG without executing), `get_planning_config`
  / `update_planning_config` (model, timeout, fallback; validated +
  evidence-logged). UI: DAG plan panel on the Agents page (steps, levels,
  validation status, method, Approve/Reject wired to the approval queue).
- `DelegationRequest.budget_usd`: `plan_delegations` stores each child's
  equal share (`None` parent → uncapped children). At spawn the desktop
  reserves the share (`cost_tracker::reserve_budget` → `budget_reservations`
  row + `task_budget` cap on the child, so the LLM gateway and tool executor
  deny over-cap calls automatically) and settles on completion (unused share
  returns; overspend appends `budget.child_exceeded`).
- Overspend surfaces: reservation-aware `check_child_budget` gates child
  execution, `settle_budget` flags post-completion overspend, and live tool
  denials with an active reservation emit `ChildBudgetExceeded`.
- `tool_runtime::tools_exec`: timeout kills are explicit and reaped —
  `kill_on_drop(true)` backstop, `kill()` before returning, Unix process-group
  kill (`setsid` at spawn + `kill -KILL -<pgid>`) / Windows `taskkill /T /F`,
  pipe drain with timeouts, `wait()` reap confirmation. Every shell receipt
  carries `processLifecycle` (`killedOnTimeout`, `killMethod`
  `direct|process_group|taskkill`, `reapConfirmed`).

## 2j. Execution fidelity model (Batch 4, live)

- Typed `ExecutionFidelity` (`mock` | `live` | `verified`) shared by
  `talos_common` (Rust) and `@talos/contracts` (`fidelity.ts`): `mock` =
  simulation, `live` = real tool execution, `verified` = live + independent
  verification (test process exit 0, hash re-verified). No layer redefines it.
- `CoderAgent`: real `fs.list → fs.create → code.run_tests → git.commit`
  through the policy broker; `verified` only when tests ran with exit 0 and
  zero failures, `live` when no framework was found, `failed` when tests fail.
- `BrowserAgent`: mock adapter (dry-run) → `mock`; live adapter
  (`TALOS_BROWSER_LIVE=1`) → `live`, upgraded to `verified` after the
  `domHash`/`screenshotHash` linkage is ledgered. `ResearchAgent` stays
  explicitly `mock` (Batch 5 adds live retrieval).
- `Supervisor::merge`: parent fidelity = min(children); `completed + mock`
  only on explicitly labeled mock paths (`debug_assert_ne!` on live paths);
  `mock`-as-`verified` is `FidelityMismatchError` (`check_fidelity_claim`).
- Persistence: `runtime_tasks.fidelity` (migration 0009) + `set_task_fidelity`;
  every `ToolReceipt` / `BrowserReceipt` / `ComputerReceipt` / `ArtifactRef` /
  `DelegationResult` / `TaskRecord` carries `executionFidelity`.

## 2k. Signed updater foundation (Batch 4, live)

- `apps/desktop/src-tauri/src/updater.rs`: `check_for_updates` (queries the
  staged-rollout endpoint, reports availability, never downloads),
  `get_update_status` (channel + rollout cohort + staged bytes),
  `download_update` (verified stage to temp), `install_update` (staged or
  fresh install → restart), `verify_update_manifest` (offline feed
  validation), and `get/set_update_channel`. The plugin verifies the
  bundle signature against the pubkey pinned in `tauri.conf.json`
  (`plugins.updater.pubkey`) before touching the installation; a bad
  signature aborts the install. 96 IPC commands total.
- `crates/talos_updates` (new): channel parsing, endpoint expansion,
  manifest parsing/validation, minisign verification
  (`minisign-verify`, same semantics the Tauri updater enforces),
  deterministic staged-rollout gating per stable `install_id`, and
  channel/install-id persistence helpers — all covered by workspace tests
  (verify + tamper/wrong-key/garbage rejection + cross-implementation
  Node-signer ↔ Rust-verifier vectors).
- `scripts/sign-release.mjs` (dependency-free, Node 26): `genkey` (ed25519
  keypair, minisign-compatible `.pub`/`.key` format, `0600` on the secret
  key), `sign` (writes `<bundle>.sig`: file signature + timestamped trusted
  comment + global signature over `filesig || trusted-comment body`),
  `verify` (used by CI smoke tests: round-trip, tamper rejection,
  wrong-key rejection). The private key must
  never be committed — passed via `--key <file>` outside the repo or
  `--key-env` (default `TALOS_UPDATER_PRIVATE_KEY`).
- `tauri.conf.json`: `plugins.updater` with staged-rollout `endpoints`
  (placeholder `https://updates.talos.dev/...` until the first release
  channel exists) + pinned dev pubkey + `createUpdaterArtifacts: true`;
  production bundling (NSIS + MSI, DMG, AppImage + DEB), per-OS signing
  config, LICENSE/README resources, and the CLI sidecar staging flow
  (`scripts/stage-cli-binaries.mjs`; see `docs/INSTALL.md`).
- UI: TopBar update banner (`UpdateBanner`: available → Download →
  Install & restart, staged bytes install without re-download) + Settings
  → Updates (`UpdatesSection`: status pill, version span, channel selector,
  Check for updates, staged download/install, restart notice; tested with
  mocked bridge).
- First-run onboarding wizard (`apps/ui/src/features/onboarding/`): six
  steps (welcome → provider → workspace → safety → demo task → done),
  persisted in shared `app_state` (`get/set_onboarding_state` IPC),
  replayable from Settings, keyboard + screen-reader accessible.
- Headless CLI (`crates/talos_cli`, `talos` binary): same policy broker +
  evidence store as the desktop, JSONL event contract + exit codes 0/1/2
  (see `docs/CLI.md`).
- Contracts: `updater.ts` (`UpdateInfoSchema`: available, current/latest
  version, notes, pubDate, downloadUrl). UI status views extend it
  bridge-locally (`UpdateStatusViewSchema`).
- Residual: staged-rollout server behavior is future work; the signing +
  verification + UI plumbing is real and tested. Downgrade protection
  (`require-version-in-signature` semantics) rides on the signature's
  trusted comment when the release channel goes live.

## 2l. MCP client/server + open-standards interoperability (Batch 4, live)

- `crates/mcp_client`: JSON-RPC 2.0 over stdio/SSE (`initialize`,
  `tools/list`, `tools/call`, `resources/list`, `resources/read`,
  `prompts/list`, `prompts/get`; protocol `2024-11-05`); `.talos/mcp.json`
  config (stdio `command`/`args`/`env`, SSE `transport: "sse"` + `url`,
  declared `capabilities`, 30s default timeout); `tools/call` builds a
  `NetworkRequest`/Medium `ActionRequest` evaluated by the policy broker
  before any byte reaches the server; receipts ledgered as `mcp_tool_call`
  (hashes only); stdio children killed + reaped on timeout.
- `crates/mcp_server`: Talos as MCP server over stdio framing — bearer-token
  auth (constant-time, `-32001` otherwise), `tools/list` over the 33
  built-ins, incoming `tools/call` authorized via `capability_for` /
  `risk_for` + policy broker (non-`Allow` denies without execution).
- `crates/instruction_parser`: deterministic bounded parsers — AGENTS.md
  (global + per-agent scopes, tool allow/deny, model prefs), CLAUDE.md
  (6-level hierarchy, build/convention/gotcha/constraint), SKILL.md
  (frontmatter + steps → quarantined skill payload), HEARTBEAT.md
  (schedules → `ScheduledTask` definitions; execution in Batch 5) — plus
  plugin manifests (ed25519 `verify_signature`, `review_manifest`,
  `approval_evidence`).
- `context_assembler::instructions`: AGENTS.md/CLAUDE.md merge into
  `safety_rules` (verbatim, never compacted); SKILL.md procedures become
  `protected + safety_critical` tool-layer segments (TypeCompact-verbatim);
  malformed files skip with a recorded reason, never fatal.
- Desktop (`mcp.rs`, 8 IPC commands + 5 events): `list/add/remove_mcp_servers`
  (manifest → `pending` + `pluginApprovalRequested`), `list_mcp_tools`
  (handshake + discovery, `mcpServerConnected`), `call_mcp_tool`
  (policy-gated receipt incl. denials, `mcpToolCallCompleted`),
  `parse_instruction_file` (`instructionFileParsed`), `list_plugins`
  (servers + manifests + ledger skills with trust), `approve_plugin`
  (`plugin_approval` evidence; bad signatures fail closed).
  Unsigned plugins enable only on explicit approval.
- UI: `/mcp` route + nav (server list/status, add/remove form, tool
  discovery + call form, instruction file viewer, plugin/skill browser with
  Approve/Reject, permission-review modal); contracts `mcp.ts` is the single
  source of truth (96 IPC commands total).
- Architecture decision: [docs/adr/0011-mcp-interoperability.md](adr/0011-mcp-interoperability.md).
  Residual: ACP wire compatibility is future work (JSON-RPC framing + tool
  routing ready for the ACP transport); HEARTBEAT.md execution in Batch 5.

## 2m. Persistent gateway daemon + headless client split (Batch 5, live)

- `crates/talos_gateway` (`talos-gateway` binary + library): the single owner
  of the evidence database (exclusive SQLite WAL lock via the v0.4.0
  encryption envelope), initializing `EvidenceDb`, `PolicyBroker`,
  `ContextAssembler` config, `Supervisor` tuning, and the `ToolRegistry`.
  Headless by design: it survives desktop-window close, executing tasks and
  listening for events permanently (OpenClaw/Hermes parity).
- Transport: loopback WebSocket `ws://127.0.0.1:8080/talos`
  (`talos-gateway/1`). Strict JSON-RPC-like frames defined once in
  `packages/contracts/src/gateway.ts` (Zod) and mirrored in
  `crates/talos_gateway/src/protocol.rs` (serde): `Command` (`SubmitTask`,
  `ApprovePolicy`, `CancelTask`, `Ping`, `GetStatus`, `ForwardIpc`) and
  `Event` (`TaskStarted`, `TaskCompleted`, `ToolInvoked`, `EvidenceRecorded`,
  `PolicyRequired`, `CaptchaDetected`, `AuthRequired`, `Pong`, `Error`, `SchedulesListed`).
- Desktop refactor (`apps/desktop/src-tauri/src/gateway.rs`): the Tauri app
  is a **client**. On startup it probes the daemon socket; when absent it
  spawns `talos-gateway` as a detached background child (surviving Tauri
  exit) and retries with backoff. All 100 IPC commands route UI → Tauri →
  Gateway via `ForwardIpc` (`gateway_forward`); reads that the daemon cannot
  yet serve stay local during the transition.
- Heartbeat: 5s `Ping`/`Pong` both directions (shell task + daemon tick).
  Three missed pongs flip the link to `Disconnected`: the UI renders a
  DISCONNECTED banner (`GatewayStatusBanner`) with a **Restart Daemon**
  button (`gateway_restart`), and the browser fallback client
  (`apps/ui/src/bridge/gateway.ts`) reconnects with exponential backoff
  (1s → 30s cap).
- Crash semantics: daemon crash ⇒ socket refused ⇒ UI DISCONNECTED; no task
  state is lost because the daemon (not the window) owns the durable queue
  and the dirty-shutdown flag still drives checkpoint resume on next daemon
  start.

## 2n. HEARTBEAT scheduler: natural-language cron engine (Batch 5, live)

- `crates/scheduler` (new): deterministic tick engine over HEARTBEAT.md.
  `Trigger` (`Interval` / `Daily` / `Weekly` / `Monthly`) owns a total
  `next_after(now, last_run)` (UTC, strictly-after, leap-aware, short-month
  clamping, burst-skipping) plus `to_cron()` display. `Scheduler` owns active
  flags, last runs, and outcomes; `tick_once(now)` returns due schedules and
  advances triggers so slow callers never double-fire; paused schedules never
  fire; `trigger_schedule_now` fires even when paused.
- Grammar (`crates/instruction_parser/src/heartbeat.rs`, extended): dayparts
  (`every morning at 9am` → 09:00, evening 18:00, night 23:00, noon 12:00,
  midnight 00:00), 12h times (`daily at 9:30pm`, `weekly on friday at 5pm`),
  shorthands (`hourly`, `every hour`, `every day`), monthly
  (`monthly on the 15th at 10:00`, `on the 1st of the month`,
  `every month at 9am`; day 1 / 09:00 defaults). New `ScheduleKind::Monthly`
  - `dayOfMonth`; file grammar unchanged otherwise (one schedule line per
    task, rest is the goal payload).
- Gateway integration (`crates/talos_gateway`): `GatewayState.scheduler`
  loads `<app_data>/HEARTBEAT.md` on boot (missing file ⇒ empty set); a 30s
  daemon loop calls `run_scheduler_tick`. Each fire decomposes the goal via
  the Supervisor DAG planner (`SupervisorAgent`, keyword path), submits
  `[heartbeat] <name>` to the durable queue, evaluates a policy gate
  (`scheduler` agent; mutating verbs → High/non-reversible), and on
  `RequireHumanReview` walks `queued → planning → waiting_policy` and
  broadcasts `PolicyRequired` (desktop notification while the user is away).
  Otherwise it executes through the broker with `TaskStarted`/`TaskCompleted`
  broadcasts; every fire is ledgered (`scheduler.fired`,
  `scheduler.policy_required`) with run counts in `ScheduleStatusView`.
- Wire: `ForwardIpc` routes `list_schedules` / `toggle_schedule_active` /
  `trigger_schedule_now`; Tauri proxies the same three commands; the shell
  forwards every daemon broadcast as `gatewayDaemonEvent` (the desktop
  notification channel short-lived sockets would miss).
- Contracts (`packages/contracts/src/scheduler.ts`): `ScheduleStatusView`
  (id, name, expression, cron, active, nextTriggerAt, lastRunAt, lastStatus,
  runCount), toggle/trigger inputs, `SchedulesListed` payload, daemon-event
  frame. UI: Settings → Heartbeats panel (schedules, cron, next trigger,
  last status, Pause/Resume, Trigger now, notification list).
- Docs: [docs/HEARTBEAT.md](HEARTBEAT.md) (file shape, expression table with
  cron mappings, fire lifecycle).

## 3. Soul: Evidence vs Claims ledger + Compensation Plans

- Every agent claim must link to runnable evidence (command, output, commit, artifact)
- `docs/EVIDENCE.md` is the human-readable ledger (updated every batch)
- Machine ledger (local, live since Batch 1): hash-chained `event_log` +
  evidence chain, claims with pending/verified/rejected/conflicting
  lifecycle, audit trail (`action_requests`, `policy_evaluations`),
  compensation plans with proposed→approved→executed/failed and
  executed→rolled_back transitions
- Policy gates: write-like actions require recorded evidence; irreversible
  actions require a compensation plan — enforced via `PolicyContext` hooks,
  no bypass

Status: **ledger + compensation foundation live locally; durable runtime queue persists entries, attempts, transitions, cancellations with `task.transition` ledgering; every tool receipt persisted as `tool_receipt` evidence; every tool call checkpointed every 5 invocations (`task_checkpoint`); database encrypted at rest with WAL, encrypted backups, dirty-flag crash recovery with checkpoint resume (Batch 4, "The Hardening") — see §2h Persistence Layer**.

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

## 5. Shield: 10-Agent Audit Swarm

- 10 independent audit agents (security, privacy, reliability, performance, architecture, evidence, dependencies, UI/UX, product, telemetry)
- Batch 1 baseline: `pnpm audit`, `tsc`, `prettier`, `node --test`, CI workflow
- Full swarm: `tools/audit` (`@talos/audit-swarm`) runs all 10 agents locally with zero extra toolchain, writes `docs/reports/audit-latest.{md,json}` plus dated copies
- Batch 2 hardening: safety-default verification (tool/browser/computer execution gates, human-review gating), performance baselines (`.talos/baselines.json`) with WARN on regression and FAIL on critical regression, deterministic run metrics in every report, live dashboard on the Audit page, CI uploads reports + baselines
- Batch 4 hardening: security agent verifies at-rest encryption wiring (envelope code paths, keychain key, sealed backups) and signed-updater config (pinned pubkey, sign/verify tooling); product agent runs documentation-freshness checks (SECURITY currency, architecture/crate parity, no stale Batch 1 caveats, README feature coverage)
- Severity model: info / warning / critical; criticals fail the command (exit 1), warnings do not (exit 0)
- CI runs the swarm after build; the Tauri bundle itself is excluded (needs WebView2/bundler + display — see CI notes)

## Batch 1 boundary (delivered, v0.1.0)

In scope (delivered, v0.1.0):

- monorepo structure, pnpm, TypeScript strict base, `just` runner
- `.github/workflows/ci.yml` green baseline (Batch 2: 6-job hardened pipeline — see [docs/CI.md](CI.md))
- docs: ARCHITECTURE, PROGRESS, EVIDENCE, ROADMAP (+ Batch 2: CI, media kit, RUNBOOK, SECURITY, CONTRIBUTING)

Out of scope (Batch 1 — all delivered later):

- Tauri app code, Rust crates, UI components, RAG, ledger enforcement, 3D UI, full swarm (Batches 2–3); file-backed persistence (Batch 4)

## Batch 2–4 boundaries (all delivered)

- Batch 2 (v0.2.0): public GitHub repo with full history + tags, enterprise
  README/badges, brand/media system, issue/PR templates, CODEOWNERS,
  community health files; deeper agent runtime (queue, lifecycle,
  cancellation), fs/shell tools behind policy, browser-use foundation,
  computer-use safety layer, context budgets; hardened CI stays green; Tauri
  bundle stays local-only with documented reason
- Batch 3 (v0.3.0 "The Awakening"): BYOK LLM gateway, context assembly,
  live browser, supervised delegation, coding workspaces, semantic memory,
  HITL approvals, real computer-use driver (feature-gated), cost + telemetry
- Batch 4 (v0.4.0 "The Hardening"): OS sandbox (§2b-iii), LLM-driven DAG
  planning + execution fidelity (§2i–2j), encrypted-at-rest evidence
  database + crash recovery with checkpoint resume (§2h), signed updater
  foundation (§2k)

Non-goals (unchanged): cloud accounts, external LLM keys in-repo, mobile targets.
