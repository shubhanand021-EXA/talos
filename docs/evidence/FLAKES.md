# Talos Flake Quarantine Register

Policy: [docs/CI.md](../CI.md) ("Flake policy"). At most ONE rerun per CI run.
A rerun that goes green MUST open or append a tracked issue with the run URL,
attempt number, and failing test id — and a row here. No silent reruns.

| Test id | First seen (run / attempt) | Owner | Deadline | Status |
|---|---|---|---|---|
| `evidence_store::db::tests::encrypted_reopen_preserves_data` (`connection pool did not drain`) | [43230343032](https://github.com/shubhanand021-EXA/talos/actions/runs/43230343032) attempt 1 (+ runs 41484991105, 41487649463, 41490635438, 41491480674) | Talos (Prompt 002) | 2026-11-19 | FIXED by Prompt 002 (`drain_pool` bounded close + 3-OS `stress-shutdown` job) |

Quarantine rules: a row stays OPEN at most 7 days; the fix lands in its own
prompt; the row flips to FIXED on merge. Deleting a flaky test or weakening
its assertion to force green is forbidden — quarantine with owner + deadline.
