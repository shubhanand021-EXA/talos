# ADR 0013: MIT → Apache-2.0 relicense

Date: 2026-11-12
Status: accepted

## Context

Talos ships MIT (`LICENSE`: "Copyright (c) 2026 Talos Contributors") and moves
to Apache-2.0 open-core (D2, execution in Prompt 004). The constitution
(`VISION.md` P7) and ledger (`docs/DECISIONS.md` D2) require the relicense
rationale to live in the repo so any cold-starting agent reads the same contract.

## Decision

1. Relicense the core from MIT to **Apache-2.0**.
2. Patent-grant rationale: Apache-2.0's express patent grant and retaliation
   clause protect contributors and users in ways MIT does not; MIT/Apache-2.0
   dependency compatibility is preserved.
3. Third-party notices are retained and tracked; GPL/AGPL stays out
   (license posture already enforced by the audit pipeline).
4. Git history is never rewritten: the relicense lands as a forward commit in
   Prompt 004, so provenance, SHAs, and audit trails stay intact.

## Accepted consequences

- Prompt 004 updates `LICENSE`, workspace `license` fields, and the audit
  license gate; this ADR is the decision record, not the execution.
- Pre-execution check for Prompt 004: `LICENSE` names "Talos Contributors"
  (plural), not a sole owner — the contributor audit must confirm relicensing
  authority before the forward commit lands.
- No contributor-license surprise: single public repo, decision locked here.
