# evidence_store

Append-only SQLite evidence store for Talos: hash-chained ledger on a
**durable, file-backed** database that survives app restarts and crashes.

## Durable persistence (`db.rs`, `storage.rs`, `recovery.rs`)

- Production connects with `EvidenceDb::connect_file(<app_data>/talos.db)`:
  WAL mode (`PRAGMA journal_mode=WAL`), foreign keys on, 5s busy timeout,
  `NORMAL` synchronous; all 7 embedded migrations run idempotently and are
  mirrored into `schema_version`.
- `connect_memory()` is **tests only** — anything written there vanishes on
  drop. The desktop shell never uses it in production.
- Layout (`storage::ensure_layout`, `TALOS_DATA_DIR` override for tests):
  `<app_data>/talos.db`, `<app_data>/backups/`, `<app_data>/models/`.
  The resolved path is logged at startup (path only, never content).
- Read-then-write transactions use `BEGIN IMMEDIATE` so concurrent writers
  serialize at `BEGIN` instead of deadlocking on the SHARED → RESERVED
  upgrade (both sides would otherwise exhaust the busy timeout).
- Backup: `VACUUM INTO <app_data>/backups/talos-<timestamp>.db`, retention
  keeps the newest 5. Restore: the backup is validated (SQLite magic,
  read-only open, identical migration set — schema drift is refused), then
  every user table is copied inside one transaction on a single connection
  (`BEGIN IMMEDIATE`, FK off, `ATTACH`, per-table `DELETE` + `INSERT`,
  `COMMIT`, then `DETACH` + FK back on). The live file, pool, and WAL mode
  are untouched; only content changes. The post-restore chain is verified.
- Startup recovery (`run_startup_recovery`, runs on every `connect_file` via
  `AppState::new_persistent`): verifies the event-log chain (a broken chain
  is reported via `startup_integrity_broken` — never a startup refusal),
  then reconciles crash-interrupted tasks: `executing` → `failed`
  (`interrupted_by_restart`); `waiting_policy` → `failed`
  (`approval_lost_on_restart` — approvals live in the in-memory
  `ApprovalStore`, so nothing pending survives a restart); `waiting_evidence`
  / `waiting_compensation` → `failed`; `canceling` → `canceled`;
  `planning` → `queued` (`requeued_after_restart`, via the recovery-only
  `force_transition`); `queued` stays queued. Every recovery is journaled,
  ledgered (`task.transition` + `startup_recovery` summary events).

## Schema (`migrations/`)

- `0001_init.sql` — legacy `events`, `evidence_records`, `claims`
- `0002_ledger.sql` — `event_log` (hash chain), claim `status` + `claim_evidence` links, `action_requests` + `policy_evaluations` audit trail, `compensation_plans` + `compensation_steps`
- `0003_runtime.sql` — durable queue: `runtime_tasks`, `runtime_attempts`, `runtime_transitions`, `runtime_cancellations`
- `0004_llm_gateway.sql` — usage + interaction-hash ledger, task spend caps
- `0005_delegation.sql` — parent links for the supervisor hierarchy
- `0006_skills.sql` — quarantined → validated → published skill registry
- `0007_cost_records.sql` — real cost records
- `schema_version` (created by `connect_file`, not a migration file) mirrors `_sqlx_migrations` so tooling can read the applied version without sqlx internals.

## Integrity model

- Every `event_log` row carries `seq`, `prev_hash`, and `hash = sha256(seq|id|kind|payload|prev|created_at)` (genesis `GENESIS`).
- Every new evidence row joins its own chain (`seq`/`prev_hash`).
- `verify_chain` / `verify_evidence_chain` recompute every link; `get_integrity_report` bundles both chains plus table counts.
- Status columns (`claims.status`, `evidence_records.status`, plan/step `status`) change only through the transition functions in `ledger.rs` — the explicit exception to append-only.

## Key functions

`connect_file`, `backup`, `list_backups`, `restore`, `run_startup_recovery`,
`append_event`, `verify_chain`, `get_integrity_report`, `create_claim`,
`attach_evidence_to_claim`, `evaluate_claim_status`, `transition_evidence_status`,
`record_action_request`, `has_compensation_plan`, `has_evidence_for_task`,
`create_compensation_plan`, `transition_compensation_plan`, `transition_compensation_step`,
`list_recent_claims`, `list_recent_log`, `list_compensation_plans`, `list_plan_steps`.

```sh
cargo test -p evidence_store
```
