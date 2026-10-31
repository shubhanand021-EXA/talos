# evidence_store

Append-only SQLite evidence store for Talos, now with a hash-chained ledger.

## Schema (`migrations/`)

- `0001_init.sql` — legacy `events`, `evidence_records`, `claims`
- `0002_ledger.sql` — `event_log` (hash chain), claim `status` + `claim_evidence` links, `action_requests` + `policy_evaluations` audit trail, `compensation_plans` + `compensation_steps`

## Integrity model

- Every `event_log` row carries `seq`, `prev_hash`, and `hash = sha256(seq|id|kind|payload|prev|created_at)` (genesis `GENESIS`).
- Every new evidence row joins its own chain (`seq`/`prev_hash`).
- `verify_chain` / `verify_evidence_chain` recompute every link; `get_integrity_report` bundles both chains plus table counts.
- Status columns (`claims.status`, `evidence_records.status`, plan/step `status`) change only through the transition functions in `ledger.rs` — the explicit exception to append-only.

## Key functions

`append_event`, `verify_chain`, `get_integrity_report`, `create_claim`,
`attach_evidence_to_claim`, `evaluate_claim_status`, `transition_evidence_status`,
`record_action_request`, `has_compensation_plan`, `has_evidence_for_task`,
`create_compensation_plan`, `transition_compensation_plan`, `transition_compensation_step`,
`list_recent_claims`, `list_recent_log`, `list_compensation_plans`, `list_plan_steps`.

```sh
cargo test -p evidence_store
```
