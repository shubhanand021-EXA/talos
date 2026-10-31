# ADR 0005: Public repository transition (pre-launch)

Date: 2026-10-31
Status: accepted

## Context

Talos was developed in a private repository under a "keep private until
launch" plan. On 2026-10-31 the Boss decided to make the repository public
effective immediately. That decision retires the launch-date privacy plan:
there is no remaining "re-privatize at/before launch" step, and no
private-until-launch statement elsewhere in the tree should be read as
operative.

Verification: `gh repo view --json visibility,name,owner`
→ `visibility: PUBLIC`, `name: talos`, owner `shubhanand021-EXA`.

## Decision

1. The canonical remote `https://github.com/shubhanand021-EXA/talos` is
   **public** from this ADR forward. Repository visibility MUST NOT be
   changed in either direction by Hotfix HF-03 or any follow-up without an
   explicit new Boss decision recorded in a superseding ADR.
2. All "private until launch" / "private repo" / collaborator-only badge /
   billing-block narratives in `README.md`, `docs/CI.md`,
   `docs/media/README.md`, `SECURITY.md`, `SUPPORT.md`, `docs/PROGRESS.md`,
   `docs/EVIDENCE.md`, `CHANGELOG.md`, `docs/ARCHITECTURE.md`,
   `docs/ROADMAP.md`, and `.github/workflows/README.md` are superseded by
   this ADR. Historical entries remain as history; operative statements
   describe a public repo.
3. Badge posture returns to live dynamic badges (CI, release/version,
   license, plus tasteful stars/forks). No static placeholder badges and no
   private-repo badge caveats remain.
4. The prior private-repo included-compute exhaustion is recorded as a
   neutral historical lesson ("private included compute exhausted; resolved
   by visibility change"). Billing, spending limits, payment methods, and
   private-usage-limit recovery are explicitly out of scope and are never
   touched by this transition.

## Accepted consequences

- Source, full git history, and tags are public and crawlable. No secret,
  credential, financial, machine-identifying, or personal data may be
  committed; history is append-only (no rewrite without explicit Boss
  approval recorded separately).
- GitHub Actions standard-runner minutes for this public repository are
  free and unlimited per GitHub policy; the previous private-repo minute
  economics no longer apply. Quality-degrading minute-saving workarounds
  from the private era are removed; good engineering (caching, concurrency
  cancellation, docs-only path filters, artifact retention caps, timeout
  limits) is kept.
- Social preview, branch protection rulesets, and any API-unavailable
  presentation steps fall back to documented manual Boss steps; automation
  never claims what it did not verify.
- This ADR supersedes any prior "private until launch" statement in docs.
