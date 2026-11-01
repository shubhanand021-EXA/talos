# Talos Security Notes (Batch 1)

Threat model: a local-first desktop agent shell. No network services, no cloud accounts, no remote telemetry in Batch 1.

## Enforcement points

- **Policy broker is the only authority** for side effects (`crates/policy_broker`). Default deny; write-like actions require recorded evidence; irreversible actions require a compensation plan (`PolicyContext` hooks, no bypass paths).
- **Tauri least privilege** (`apps/desktop/src-tauri/capabilities/default.json`): the main window gets zero filesystem, shell, HTTP, dialog, or notification permissions. Production CSP allows only `self` + Tauri `ipc:`/`asset:` schemes.
- **Append-only ledger** (`crates/evidence_store`): hash-chained `event_log` + evidence chain (SHA-256), claim lifecycle with explicit transitions, tamper-detection tests. Status changes go through repository transition functions — never ad-hoc edits.
- **No secrets in repo**: the audit swarm scans for API keys, private keys, and tokens on every run (pattern definitions and test fixtures are excluded by design; production paths are scanned).

## What is NOT yet secured

- The SQLite store is file-backed (`<app_data>/talos.db`, WAL, idempotent
  migrations, startup integrity check + crash recovery, `VACUUM INTO`
  backups with newest-5 retention) — but there is **no at-rest encryption
  yet**; anyone with filesystem access can read `talos.db`. Full-disk
  encryption is the current mitigation; encrypted-at-rest storage (SQLCipher
  or equivalent) is future work.
- The echo agent auto-verifies its own diagnostic evidence (mock trust — a real verifier arrives later).
- No sandboxing of executed commands beyond policy gating; computer-use arrives in Batch 2 with human-review gates.
- Icons are generated placeholders; replace before release signing.

## Reporting issues

Do **not** report suspected vulnerabilities in public issues. Use the private channel in [SECURITY.md](../SECURITY.md) (`security@talos.dev` placeholder or `Security → Report a vulnerability`). Open a regular issue with the audit report attached (`docs/reports/audit-latest.md`) only for non-security findings. Do not post suspected secrets — rotate them first.
