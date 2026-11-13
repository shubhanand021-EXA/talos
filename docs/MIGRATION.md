# Migration kit — import from other agent tools

The migration kit imports instruction files, skills, schedules, and MCP
server configs from other agent tools into Talos constructs. Imported
content is **untrusted data, never authority**: everything lands
quarantined or disabled, nothing executes, and enabling an imported item
is always a separate approval afterwards.

## What imports

| Source                            | Talos construct                        | Default state | Re-enable path (separate approval) |
| --------------------------------- | -------------------------------------- | ------------- | ---------------------------------- |
| `AGENTS.md` / `CLAUDE.md`         | context-assembler directive candidates | quarantined   | n/a — data only                    |
| `SKILL.md`                        | skill-extractor quarantine             | quarantined   | `validate_skill` → `publish_skill` |
| `HEARTBEAT.md`                    | scheduler job candidates               | disabled      | `toggle_schedule_active`           |
| `.mcp.json` / `mcpServers` blocks | mcp_client server entries              | disabled      | `approve_plugin`                   |

Flow: **scan** (read-only detect + parse) → **preview** (dry-run diff of
exactly what would be created) → **per-category human approval** →
**import** (quarantined/disabled, idempotent — re-running creates nothing).
Every step writes an evidence receipt (`migration.scan`, `migration.preview`,
`migration.import`).

## What NEVER imports

- **Secrets.** `api_key` / `secret` / `password` / `token` / `sk-…` /
  `ghp_…` values are stripped at scan time (`secretStripped: true`).
  Re-enter credentials via the OS keychain (`talos_secrets`); plaintext
  files are never trusted.
- **Permissions or grants.** No `allow:` list becomes an enablement.
- **Execution.** Scan, parse, preview, and import never run imported
  content. Unknown directives are preserved verbatim as quarantined data —
  flagged, never interpreted as shell.
- **Auto-enable.** The importer cannot enable tools, servers, jobs, or
  bypass `policy_broker`. There is no code path from import to enablement.

## The injection fence

Imported text is scanned for prompt-injection and destructive patterns
(`ignore all policy`, `bypass`, `grant all permissions`, `rm -rf`, …).
Matches are labeled (`prompt-injection`, `destructive-directive`,
`secret-present`) and the item imports quarantined/disabled like
everything else — the label only adds review signal. Fence behavior is
pinned by negative tests with synthetic fixtures (no copyrighted content).

## Re-enable flow (separate approvals)

1. Review the quarantined item (preview diff + risk labels).
2. Approve through the existing trust path: `approve_plugin` for MCP
   servers, `validate_skill` → `publish_skill` for skills,
   `toggle_schedule_active` for schedules.
3. The migration panel never calls these paths itself; it links to them.

## Where in the UI

Settings → **Migration kit** (scan, preview diff, per-category approve,
quarantine status), plus a pointer on the onboarding Done step. The panel
announces scan/preview/import results via a live region and keeps full
keyboard operability (native inputs, focus-managed confirm dialog).
