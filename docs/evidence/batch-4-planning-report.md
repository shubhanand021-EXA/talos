# Talos Batch 4 — LLM-Driven DAG Planning: Evidence Report (TAL-003)

Date: 2026-11-02 (UTC). Status: implemented; verification recorded below.

## 1. What was built

Replaced the deterministic keyword-only task decomposition with LLM-driven DAG
planning validated by deterministic policy gates (`TAL-003 P1`).

- **`crates/agent_runtime/src/planner.rs`** (new): `PlanningPromptTemplate`
  (deterministic, versioned `1.0`; goal, roles/capabilities, available tools,
  budget, max depth/fan-out, strict
  `{"plan_version","steps","total_estimated_tokens","reasoning"}` schema);
  `parse_plan_json` (markdown-fence tolerant, unknown-fields rejected);
  `validate_plan` gates — schema, dependency existence, acyclicity
  (topological sort), depth (longest chain ≤ `max_depth`), fan-out (widest
  level ≤ `max_fanout`), tools (in-registry AND in-role-scope), budget
  (total equals step sum, fits token budget), risk (`high` →
  `requires_approval`); `topological_order`, `execution_levels`,
  `failed_dependents`, `keyword_fallback_dag`, `validated_to_dag`,
  `planning_evidence_payload` + `plan_artifact` (metadata/hashes only).
  13 unit tests.
- **`crates/agent_runtime/src/supervisor.rs`**: `SupervisorAgent::plan_with_llm`
  (model via `PlanningLlmClient`, single correction retry, 30s timeout, no-key
  fast path; every failure falls back to keyword decomposition labeled
  `PlanningMethod::Keyword` with a recorded reason; request/response hashes +
  token estimates via `context_assembler::estimate_tokens`); `PlanningConfig`
  (model/timeout/fallback, validated); `plan_dag_delegations` (topological
  order, `{parent}-step-{step_id}-{role}` ids); `planning_events`
  (`PlanGenerated`/`PlanValidated`/`PlanFallback`); `token_budget_from_usd`
  ($1 ≈ 100k tokens, documented). 4 new tests. `RuntimeEvent` gains the 4
  plan variants (`lib.rs`).
- **Desktop** (`apps/desktop/src-tauri`): `GatewayPlanningClient` (policy-gated
  gateway call, purpose `supervisor planning`); `plan_for_parent` (prompt →
  validate → `plan_artifact` evidence + `planning.completed` ledger event +
  `planGenerated`/`planValidated`/`planFallback` emissions);
  `preview_plan` (DAG without executing), `get/update_planning_config`
  (validated + ledgered), `planning_config` state, 3 new commands registered
  (`lib.rs`, count 65 → 68); `delegate_task_inner` plans via LLM (explicit
  role still single-child keyword), holds high-risk plans for HITL
  (`awaiting_approval`, `approvalRequested`, zero spawns), executes levels in
  topological order with dependent cancellation (`canceled`, no tools run).
  4 new tests (`preview_plan_does_not_execute`,
  `update_planning_config_validates_and_applies`,
  `high_risk_plan_holds_for_approval`, `blocked_dependent_is_canceled_without_tools`).
- **Contracts** (`@talos/contracts`): `PlanningMethod`, `PlanStepView`,
  `PlanPreviewView`, `PlanningConfigView`, `PreviewPlanInput`,
  `UpdatePlanningConfigInput` schemas; `previewPlan/getPlanningConfig/
  updatePlanningConfig` commands; `planGenerated/planValidated/planRejected/
  planFallback` events; stability tests updated.
- **UI**: `features/planning/PlanViewer` (DAG steps in topological order,
  levels, method/validation pills, fallback reason, per-step risk/tokens,
  Approve/Reject buttons; 3 tests) + Agents "DAG plan preview" panel wired to
  `previewPlan` and the approval queue; bridge `previewPlan/
  getPlanningConfig/updatePlanningConfig`.
- **Docs**: `ARCHITECTURE.md` §2i planning block, `CHANGELOG.md` TAL-003 entry,
  desktop README command surface (52 → 55).

## 2. Requirement coverage

1. Planning prompt template — `planner.rs::PlanningPromptTemplate` ✅
2. LLM planning call (`plan_with_llm`, ContextAssembler token estimates,
   parse → 1 correction retry → keyword fallback with warning event,
   evidence hashes + cost via gateway usage records) ✅
3. Deterministic validation gates (schema/dependency/depth/fan-out/tool/
   risk/budget; rejection + fallback + reason recorded) ✅
4. DAG execution (parallel levels bounded by fan-out, dependency wait,
   failed-dependency cancellation, topological order, per-step child tasks,
   `plan_artifact` evidence) ✅
5. Fallback and safety (no key / >30s timeout / double-invalid JSON →
   keyword labeled `planning_method`; policy broker still gates every step) ✅
6. Desktop IPC (`preview_plan`, `get/update_planning_config`;
   `planGenerated/planValidated/planRejected/planFallback`) ✅
7. UI plan viewer (DAG graph list, role/goal/deps/risk/tokens, validation
   status, method label, Approve/Reject → HITL) ✅
8. Testing (Rust 17 new + TS 6 new; see §3) ✅
9. Docs + commit/PR (this report; commit
   `feat(planning): add LLM-driven DAG planning with deterministic validation`) ✅

## 3. Verification results (all green)

- `cargo test -p agent_runtime`: **91 passed** (13 planner + 4 supervisor planning).
- `cargo test -p talos-desktop`: **68 passed** (10 delegation incl. 4 new planning).
- `cargo clippy -p agent_runtime`: clean; `cargo check -p talos-desktop`: clean.
- `@talos/contracts` vitest: **77 passed** (plan schemas + IPC stability).
- UI vitest: **130 passed** (PlanViewer 3 + Agents/bridge suites).
- `pnpm typecheck`: clean.
- Prettier: new UI planning files clean (tree-wide CRLF warnings are a
  Windows-checkout artifact; HEAD files warn identically).

## 4. Known limitations

- Within-level steps execute sequentially (bounded by the validated fan-out);
  true parallel execution is future work — ordering guarantees already hold.
- `token_budget_from_usd` assumes $0.01/1k blended rate; callers with exact
  model pricing should pass an explicit token budget.
- High-risk hold requires re-invoking `delegate_task` after grant (no
  auto-resume) — documented in `hold_for_plan_approval`.
- One full-suite desktop run showed a single parallel-test flake
  (`delegates_multi_domain_to_multiple_children`); immediate reruns green.
