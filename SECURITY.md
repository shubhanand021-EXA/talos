# Security Policy

## Supported versions

| Version | Supported          |
| ------- | ------------------ |
| 0.4.x   | :white_check_mark: |
| < 0.4.0 | :x: (upgrade)      |

Only the latest release line receives security fixes. Talos is pre-1.0 and
moving fast; older lines are not patched — upgrade to the latest tag.

Talos is local-first in Batch 4: no network services, no cloud accounts, no remote telemetry. Security boundaries are the policy broker, Tauri least-privilege shell, and the append-only evidence ledger.

## Reporting a vulnerability

**Do not report vulnerabilities in public issues.**

- Primary channel: GitHub **private vulnerability reporting**
  (`Security → Report a vulnerability`) on this public repository
  (verified enabled). This keeps the report confidential until a fix ships.
- Include: affected version/commit, reproduction steps, impact assessment,
  and the audit report (`docs/reports/audit-latest.md`) if relevant.
- Do not include secrets or live credentials in the report.

## Coordinated disclosure (90 days)

- We acknowledge receipt within 72 hours, triage severity, and coordinate a
  fix + disclosure timeline with you.
- Please give us reasonable time to patch before any public disclosure, up
  to 90 days from acknowledgement for the initial fix; if a fix needs
  longer, we will agree a revised date with you in writing.
- After the fix is released, we publish a security advisory crediting the
  reporter (unless anonymity is requested).

A dedicated contact email on a Talos-owned domain arrives when Owner Task
O-1 (public brand) closes; until then, private vulnerability reporting is
the only accepted channel. There is deliberately no email contact listed
here — any talos-named email address you may have seen in older docs belongs
to an unrelated third party and is not ours.

## Scope and enforcement points

- **Policy broker is the only authority** for side effects (`crates/policy_broker`). Default deny; write-like actions require recorded evidence; irreversible actions require a compensation plan (`PolicyContext` hooks, no bypass paths).
- **Tauri least privilege** (`apps/desktop/src-tauri/capabilities/default.json`): main window gets zero filesystem, shell, HTTP, dialog, or notification permissions. Production CSP allows only `self` + Tauri `ipc:`/`asset:` schemes.
- **Append-only ledger** (`crates/evidence_store`): hash-chained `event_log` + evidence chain (SHA-256), claim lifecycle with explicit transitions, tamper-detection tests.
- **No secrets in repo**: the audit swarm scans for API keys, private keys, and tokens on every run.

## Known limitations (Batch 1 baseline)

- SQLite store is in-memory; no at-rest encryption or file-backed persistence yet.
- Echo agent auto-verifies its own diagnostic evidence (mock trust — a real verifier arrives later).
- No sandboxing of executed commands beyond policy gating; computer-use arrives in Batch 2 with human-review gates.
- Icons are generated placeholders; replace before release signing.

See also: [docs/SECURITY.md](docs/SECURITY.md) (threat model detail) · [docs/RUNBOOK.md](docs/RUNBOOK.md) (ops) · [docs/CI.md](docs/CI.md) (audit gates).
