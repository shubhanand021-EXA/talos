# Talos Decision Ledger

Status: Locked 2026-11-12. Owner: Shubh Anand.

This ledger records D1–D12 exactly as locked 2026-11-12. Substance must not be
altered; ambiguities are flagged UNKNOWN in the batch report, never invented.
Constitution: `VISION.md`. Process: `docs/CI.md`. Standing orders: `AGENTS.md`.

Source-of-truth hierarchy: repo code + tests → `VISION.md` / `docs/DECISIONS.md`
→ `docs/parity/CAPABILITY_MATRIX.json` (Prompt 009, not yet present) →
architect prompts → nothing else.

## D1 — HYBRID (continue v0.4.0)

Decision: Continue v0.4.0; edward patterns salvaged later; anvilpoint = audit engine.
Rationale: Preserve working momentum; salvage edward patterns incrementally;
anvilpoint serves as the audit engine.
Status: Locked 2026-11-12.
Repo anchor: `reports/002-drain-pool-fix.md` (v0.4.0 line; release model in
`CHANGELOG.md`).

## D2 — Apache-2.0 open-core

Decision: Apache-2.0 open-core.
Rationale: Patent grant, ecosystem compatibility, commercial usability.
Status: Locked 2026-11-12; EXECUTED in Prompt 004 (`LICENSE` =
full Apache-2.0 text, all manifests `Apache-2.0`, NOTICE added;
history not rewritten).
See `docs/adr/0013-apache2-relicense.md`.

## D3 — Rename forced; "Talos" internal codename

Decision: Rename forced; "Talos" is the internal codename; public brand gated on
Owner Task O-1; no public marketing or signed release before O-1 closes.
Rationale: Name collision (see ADR-0014); public brand requires owner decision O-1.
Status: Locked 2026-11-12; O-1 open.
See `docs/adr/0014-codename-public-brand.md`.

## D4 — Tauri 2 + Rust + React 19 stack

Decision: Tauri 2 + Rust + React 19; NO crate renames.
Rationale: Native shell + shared Rust policy core + modern UI; crate renames
would break provenance and audit trails.
Status: Locked 2026-11-12.
Repo anchor: Tauri 2 (`apps/desktop/src-tauri/Cargo.toml`, `tauri-build 2`);
React 19.3.1 (`apps/ui/package.json`); workspace members stable in
`crates/Cargo.toml` (hard prohibition on renames, `AGENTS.md` §3).

## D5 — Wedge: coding + browser + evidence first

Decision: Wedge is coding + browser + evidence first.
Rationale: Highest-leverage vertical slice; evidence differentiates every claim.
Status: Locked 2026-11-12.
Repo anchor: ADR 0001 (browser-use) + ADR 0002 (computer-use safety layer) +
`crates/evidence_store`.

## D6 — BYOK + local models only

Decision: BYOK + local models only; no consumer OAuth at launch; quarterly policy review.
Rationale: Key custody stays with the user; avoids OAuth scope and policy-volatility risk.
Status: Locked 2026-11-12; quarterly review due.
See `docs/adr/0015-byok-auth-fence.md`. Repo anchor: ADR 0006
(`crates/llm_gateway`, `crates/talos_secrets` OS keychain); OAuth-subscription
ban is a hard prohibition (`AGENTS.md` §3).

## D7 — Local-free charter

Decision: Local-free charter (= P10 of `VISION.md`).
Rationale: Every local feature free forever; trust anchor for local-first adoption.
Status: Locked 2026-11-12; enforced as AUTOMATIC ZERO in every audit.

## D8 — Pricing hypotheses

Decision: Pricing hypotheses $0 / $19 / $39 / $99 + $2,500/mo enterprise floor,
pending waitlist validation.
Rationale: Hypothesis only — validates willingness to pay before committing.
Status: Locked 2026-11-12; UNVALIDATED (waitlist pending). Not a price promise.

## D9 — Governance

Decision: Per-prompt DoD, 10-prompt batches, ~600-line diff cap (merges exempt),
AI never touches signing private keys or production credentials, rollback by
green tag only.
Rationale: Small reviewable batches; signing keys and prod creds stay
human-only; rollback always lands on a known-green tag.
Status: Locked 2026-11-12. DoD mirrored in `CONTRIBUTING.md`; diff cap and
no-force-push-`main` in `AGENTS.md` §3–§4.

## D10 — Weekly build-in-public, gated on O-1

Decision: Weekly build-in-public, gated on O-1.
Rationale: Public cadence starts only after the public brand decision closes.
Status: Locked 2026-11-12; gated (O-1 open). No public marketing before O-1 closes (D3).

## D11 — DeepSeek Harness = 8th primary competitor

Decision: DeepSeek Harness = 8th primary competitor; re-verify desktop status
before launch.
Rationale: Competitive set stays current; desktop-status claims decay fast.
Status: Locked 2026-11-12; re-verification owed before launch (UNKNOWN until done).

## D12 — `talos-agent` org TAKEN — struck

Decision: `talos-agent` org TAKEN — struck; fresh name/org/domains via O-1.
Rationale: Avoid squatted-namespace collision; resolve naming inside O-1.
Status: Locked 2026-11-12 (struck option); replacement pending O-1.
Repo anchor: canonical remote is `shubhanand021-EXA/talos`, public (ADR 0005).

## Cross-links

- Vision/pillars: `VISION.md` (P10 restates D7).
- ADRs: `docs/adr/0013-apache2-relicense.md` (D2),
  `docs/adr/0014-codename-public-brand.md` (D3, D12),
  `docs/adr/0015-byok-auth-fence.md` (D6).
- Process: `docs/CI.md`. Standing orders/DoD: `AGENTS.md`, `CONTRIBUTING.md`.

## D11 — Nuance recorded 2026-11-14 (status stays locked; substance only)

The D11 block above is preserved byte-for-byte as locked 2026-11-12.
Architect live verification 2026-11-14 overturns its premise:
DeepSeek Harness NOW ships an official desktop app (macOS Apple Silicon

- Windows, Electron, MIT, dsh-0.2.2 merged 2026-11-13 —
  deepseek.com/harness, github deepseek-ai/deepseek-harness apps/desktop);
  OpenClaw ships a native Windows Hub + cross-platform Tauri desktop app
  (docs.openclaw.ai/platforms/windows). The "first desktop" window is
  CLOSED. The wedge re-positions from "ship the desktop first" to verified
  execution + evidence ledger + default-deny governance + multi-provider
  BYOK neutrality + enterprise audit + Tauri-light performance. D11's
  status (locked) and decision text are unchanged; this note records the
  evidence update only. Threat-register entries T1/T3 reset with the same
  date in `docs/THREAT_REGISTER.md`.
