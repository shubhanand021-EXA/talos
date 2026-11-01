# agent_runtime

Deterministic orchestration for the Talos local control plane. No network by
default: `LocalOrchestrator` runs the Batch 1 diagnostic loop, the durable
queue executes tasks through the policy broker, and the Batch 3 Supervisor
decomposes complex work into scoped sub-agents.

```sh
cargo test -p agent_runtime
```

## Modules

- `lifecycle`: strict 11-state machine (`queued → planning → executing → … →
completed/failed/timed_out`, `canceling → canceled`, explicit retry).
- `queue`: SQLite-backed durable queue (`submit/get/list/cancel/retry`,
  priority ordering, timeouts, retries, cooperative cancellation,
  `run_due_tasks`). Batch 3 adds `parent_task_id`, `submit_task_with_id`
  (deterministic child ids), `TaskTreeView`, `get_task_tree`,
  `cancel_subtree`.
- `supervisor`: `supervise_once` (timeout/cancel/retry pass) plus
  `SupervisorAgent` (decompose → plan → policy-gate → merge; never bypasses
  the broker, never overrides a child verdict).
- `delegation`: protocol types (`DelegationRequest/Result`, `ArtifactRef`,
  `SpecialistRole`), `MAX_FANOUT=4`, `MAX_DEPTH=2` (root = 1, child = 2),
  deterministic `decompose_task` (code/browser/research keywords),
  `is_narrower_authority` (child ⊆ parent), `split_budget_equal`, `merge_results`.
- `specialists`: `Specialist` trait + `CoderAgent` (`fs.*`, `process.exec`),
  `BrowserAgent` (`browser.*`), `ResearchAgent` (read-only + extract).
  Bounded 4000-token windows, structured artifacts (never conversation dumps).
- `messages`: typed `SupervisorToAgent` (`TaskAssignment`, `CancelSignal`,
  `BudgetUpdate`) / `AgentToSupervisor` (`ProgressUpdate`,
  `ArtifactProduced`, `TaskCompleted`, `TaskFailed`) + bounded
  `DelegationBus` (no agent-to-agent traffic).
- `concurrency`: `ConcurrencyController` (default 2, hard cap 4), one writer
  per workspace path, one browser-session holder, `ConflictKind`
  (`file_lock` | `browser_busy` | `concurrency_exhausted`).
- `llm_agent`: policy-gated model calls via the LLM gateway (mock fallback
  when no provider key is configured).
