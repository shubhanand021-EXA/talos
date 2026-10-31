# evidence_store

Foundational SQLite evidence store for Talos. Append-only mindset: rows are INSERTed and SELECTed, never UPDATEd or DELETEd.

## Schema (`migrations/0001_init.sql`)

- `events(id, kind, payload_json, created_at)`
- `evidence_records(id, task_id?, agent_id?, session_id?, kind, status, label?, summary?, hash, uri?, mime_type?, payload_json?, created_at, verified_at?)`
- `claims(id, statement, evidence_ids_json, created_by_agent_id?, task_id?, created_at)`

Evidence rows carry a `sha256:` hash of their canonical JSON payload for integrity. Full ledger verification arrives later.

## Run

```sh
cargo test -p evidence_store
```
