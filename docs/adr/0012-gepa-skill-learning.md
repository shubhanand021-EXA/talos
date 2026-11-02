# ADR 0012: GEPA skill learning loop (extraction → quarantine → evolve)

Date: 2026-11-02
Status: accepted

## Context

Audit reports converge: Talos proposes skills but never closes the loop
(Meta §3.2 Hermes: distill every 15 tool calls into `SKILL.md`, recall on
similar tasks, improve via management; Grok §13.2/P1: improve skills from
verified outcomes, surface automatically in assembly; Manus §1.3:
cross-session distillation still thin). The registry
(`quarantined → validated → published`) has no usage tracking, no
improvement path, and no retrieval beyond snippet-level semantic recall.

## Decision

1. **New `crates/skill_extractor` crate** (deterministic, no LLM):
   `extract_skill_for_task` distills verified completions from the
   `tool_receipt` chain (ordered successful calls + recorded action-request
   params, truncated) into SKILL.md validated by `instruction_parser`;
   born `quarantined`, indexed for recall. `agent_runtime` invokes it
   best-effort on verified completion (never fails the task).
2. **GEPA evaluation** (`GepaConfig`, default every 15 receipts):
   watermarked (`gepa_evaluation` rows) incremental passes detect recurring
   tool sequences across distinct tasks — covered patterns record usage
   (success = completed + verified), uncovered patterns propose one
   quarantined skill. Sustained-failing published skills auto-deprecate;
   stale ones (30 days) deprecate; sub-threshold ones flag review. Desktop
   `invoke_tool` spawns the pass in the background when due; the manual
   trigger returns immediately.
3. **Skill evolution in `evidence_store`** (migration `0011_skill_stats`):
   `usage/success/failure_count`, `confidence_score` (running rate),
   `last_used_at`, `parent_version`; lifecycle extended to
   `published → deprecated` and `any → archived`; `create_skill_version`
   (`{id}-v{n}`, quarantined, parent untouched for rollback) plus a bounded
   LCS `skill_diff`.
4. **Retrieval**: `gather_published_skills` (verified recall →
   published-only + ceiling-covering filter → structured `# Skill / When to
use / Steps / Required tools` tool-layer segments) feeds
   `preview_context`; `published_skill_tools` drives a `+0.2` Tool-RAG member
   boost (`rank_with_skill_boost`, empty set ≡ baseline).
5. **Desktop IPC (9 commands) + 5 events**: `get/validate/publish/deprecate/
archive_skill`, `get_skill_stats`, `list_skill_versions`,
   `diff_skill_versions`, `trigger_skill_evaluation`; events
   `skillValidated/Published/Deprecated/Archived/EvaluationCompleted`
   (`skillCreated` already existed). UI: Skill Manager on the Memory page
   (5-status registry with usage stats, document viewer, lifecycle buttons,
   version history + diff, semantic skill search, Evaluate Now).

## Consequences

- Quarantined skills can never reach the prompt (verified-only recall +
  status check + tests); promotion stays human-gated.
- Usage statistics are ledger-local and deterministic; confidence is the
  running success rate (5+ uses at 100% → 1.0).
- Auto-deprecation is conservative (≥5 uses below 70%, or 30-day staleness);
  review flags surface earlier (≥3 uses).
- 89 IPC commands total (was 80); `IpcCommandName`/`IpcEventName`/memory
  stability tests updated.
