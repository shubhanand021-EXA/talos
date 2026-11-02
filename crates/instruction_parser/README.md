# instruction_parser

Open-standards instruction parsers: AGENTS.md, CLAUDE.md, SKILL.md,
HEARTBEAT.md, plus the plugin-trust foundation. Every parser is
deterministic (same bytes → same value) and bounded (256 KiB inputs,
bounded item counts).

```sh
cargo test -p instruction_parser --lib
```

## Modules

- `agents`: global + `## Agent: <name>` scopes, `- allow:` / `- deny:` tool
  gates, `- model:` preferences; `instructions_for` / `allow_for` /
  `deny_for` merge global + scope.
- `claude`: 6-level hierarchy (0 = global `~/.talos/CLAUDE.md`, 1 = project
  root, 2–5 = nested); `build:` / `convention:` / `gotcha:` /
  `constraint:` categorization; `merged_instructions` /
  `merged_constraints` / `merged_build_commands` apply outermost-first.
- `skill`: `---` frontmatter (`name`, `description`, `triggers`,
  `required_tools`) + numbered-step body; `skill_id` derivation,
  `permission_ceiling` heuristic, `to_skill_payload` for the `skills` table
  (born `quarantined`).
- `heartbeat`: `every N minutes|hours`, `daily at HH:MM`,
  `weekly on <day> at HH:MM` → `ScheduledTask` definitions (execution in
  Batch 5; parsing only).
- `plugin`: `PluginManifest` (`name`, `version`, `author`, `capabilities`,
  `permissions`, `source_url`, `signature`), `review_manifest` dialog
  payload, ed25519 `verify_signature` over canonical bytes, and
  `approval_evidence` for `plugin_approval` ledger rows.

## Safety contract

Parsed instruction content is **safety-critical**: context assembly
(`context_assembler::instructions`) carries AGENTS.md/CLAUDE.md text in
`safety_rules` (verbatim, never compacted) and SKILL.md procedures in
`protected + safety_critical` tool-layer segments so TypeCompact preserves
them byte-identical. Malformed files are skipped with a recorded reason —
never fatal to assembly.
