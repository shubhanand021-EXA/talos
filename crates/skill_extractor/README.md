# skill_extractor

GEPA skill learning: deterministic extraction of reusable procedures from
verified task runs, periodic pattern evaluation, and published-skill
retrieval. No LLM anywhere — same evidence chain, same skill document.

```sh
cargo test -p skill_extractor
```

## Extraction (`extract.rs`)

`extract_skill_for_task(db, task_id)` requires `status == "completed"` and
`fidelity == "verified"`, then reads the task's `tool_receipt` chain
(oldest first), keeps successful calls, and builds a `NewSkill`:

- `required_tools`: sorted unique tool ids; `permission_ceiling`: highest
  `tool_runtime::risk_for` across them
- `procedure_text`: numbered `Call <tool> (params). Policy: <verdict>.`
  steps; params come from the recorded action request (`tool-{receipt_id}`)
  with values truncated (keys sorted, never executed)
- `triggers`: significant title words; `test_fixture_ref`: the task id

The generated SKILL.md must round-trip through
`instruction_parser::parse_skill` or extraction skips. Outcomes are
`Created(row)` (quarantined + indexed) or `Skipped { code, detail }`
(unknown / not completed / not verified / no calls / already distilled).

## GEPA loop (`gepa.rs`)

`GepaConfig::default()` reviews every 15 tool receipts. A
`gepa_evaluation` evidence row watermarks `receipt_count`; `maybe_evaluate`
returns `None` until the cadence is due (two `COUNT` queries — the hot path
never scans). `evaluate` detects identical successful-call sequences across
≥2 distinct tasks: covered patterns `record_skill_use` per task (success =
completed + verified); uncovered patterns propose one quarantined skill.
Sustained-failing published skills (`should_deprecate`) and stale ones
(30 days) auto-deprecate; sub-threshold skills land in `needs_review`.
Evaluation is spawned in the background by desktop triggers — it never
blocks tool execution.

## Retrieval (`retrieve.rs`)

`gather_published_skills(db, goal, risk_ceiling, top_k)`: hybrid recall over
verified `skill` entries → `published`-only + ceiling-covering filter →
structured `# Skill / When to use / Steps / Required tools` segments.
Quarantined skills are never retrievable. `published_skill_tools` returns
the member-tool union for Tool-RAG boosts.
