# ADR 0014: Codename vs public brand

Date: 2026-11-12
Status: accepted

## Context

"Talos" is the internal codename (D3). A forced rename is required before any
public marketing or signed release, with the public brand gated on Owner Task
O-1. The governing prompt records 5+ existing AI-agent "Talos" products,
including a permission-kernel agent with session replay — a direct collision
with this project's security/evidence positioning.

## Decision

1. **Codename:** "Talos" stays internal only (repo, code, prompts).
2. **Public brand:** fresh name decided exclusively via Owner Task O-1.
   No public marketing or signed release before O-1 closes (D3, D10).
3. **`talos-agent` org TAKEN — struck** (D12); fresh name/org/domains via O-1.
4. **Migration = GitHub transfer, never recreate:** when the public brand lands,
   transfer the repository (preserves stars, PRs, history, provenance) instead
   of recreating it. Canonical remote is `shubhanand021-EXA/talos`, public
   per ADR 0005.

## Accepted consequences

- All VISION P7 "public roadmap / devlog" work stays gated on O-1; building in
  public starts weekly only after O-1 closes (D10).
- Collision risk (confused installs, trademark friction, replay-comparison with
  the permission-kernel "Talos") is contained by keeping "Talos" out of public
  surfaces.
- Transfer runbook must be written under O-1 (transfer, not recreate).
