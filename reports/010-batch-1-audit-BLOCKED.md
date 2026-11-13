# Report 010 — Batch-1 audit: GATE BLOCKED (drain_pool tripwire tripped)

- Date: 2026-11-13 (UTC)
- Agent: OpenCode (sole engineer)
- Batch: 1 / Prompt: 010 (audit gate; BLOCKED — no tag, no merge)
- Read order followed: `AGENTS.md` → `VISION.md` → `docs/DECISIONS.md` →
  `reports/001-*` … `009-*` → `docs/CI.md` → `docs/evidence/FLAKES.md`
- Branch: `chore/batch-1-audit-prompt-010` (unmerged; pushed for the record only)

## 1. Audit table (pillar → score → evidence)

Scoring rubric (equal weights, stated upfront): each pillar 0–100 against its
VISION.md target; Unknown counts as 0 for proven completion and is flagged;
evidence must be a repo file, test id, or CI run observed in this session —
never asserted from memory.

| Pillar               | Score             | Evidence                                                                                                                                                                                                                            | One-line justification                                              |
| -------------------- | ----------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------- |
| P1 Parity            | 90                | `docs/parity/CAPABILITY_MATRIX.json` (150 rows, validated), `SUMMARY.md` (P0 30/31), CI `parity` job green (run `43420183120`, `43422920531`)                                                                                       | Matrix machine complete; single P0 gap (signed releases partial)    |
| P2 Security          | 75                | `crates/policy_broker` (14 engine tests), `scripts/sign-release.mjs` + keychain rule, CodeQL 0 critical open (3 pre-existing highs #9–#11), `ci.yml` least-privilege `permissions`                                                  | Default-deny + least-privilege hold; no signed release yet          |
| P3 Performance       | 35                | `.talos/baselines.json` (bundle/duration gates only), `FpsMeter.tsx` (measures, does not gate); NO CI gates for cold-start/≤100ms/60fps/RAM/installer/lite-mode                                                                     | Thresholds are targets without enforcement                          |
| P4 Local-first       | 85                | `crates/llm_gateway/src/providers/ollama.rs`, `docs/adr/0009-telemetry-local-first.md`, keychain-only secrets                                                                                                                       | Offline-capable + opt-in telemetry; no full offline test pass       |
| P5 AAA UX            | 50                | `apps/ui/src/features/void-neon/` (identity intact), `OnboardingWizard.tsx` (exists, timing unverified), no WCAG audit anywhere                                                                                                     | Identity present; accessibility + timing unproven                   |
| P6 Evidence          | 90                | `crates/evidence_store` (hash chain + tamper tests), `ViewportControls.tsx` replay slider + `viewport.test.tsx`, `docs/reports/audit-latest.{md,json}` + dated copies + CI upload                                                   | Ledgered, replayable, exportable                                    |
| P7 Open governance   | 75                | `LICENSE` Apache-2.0 + manifest/license charter check green, `docs/ROADMAP.md`, `reports/001-009`, no `docs/devlog/` (gated on O-1 per D10)                                                                                         | Weekly devlog absent by owner gating, not by neglect                |
| P8 Reliability       | 45, TRIPWIRE FAIL | flake policy + register + issues #34/#38/#41/#43, `stress-shutdown` green on every run incl. current, 100-cycle local stress pass; crash-free sessions unmeasured; 30-green streak not met; **2 post-merge drain occurrences (§3)** | Mechanisms enforced; metric + streak unmet; active flake class      |
| P9 Cost transparency | 60                | `docs/TELEMETRY.md` pricing table + `ModelDescriptor` catalog (ADR 0006: costs are list-price estimates), per-task caps enforced                                                                                                    | Unit costs published; ±10% estimate-accuracy gate missing           |
| P10 Charter          | 90                | No paywall exists anywhere; local-free charter in VISION P10; `CONTRIBUTING.md` DoD                                                                                                                                                 | No violation; enforcement is declarative, no automated paywall test |

**Composite: (90+75+35+85+50+90+75+45+60+90)/10 = 695/10 = 69.5 → FAIL (< 80).**
No automatic-zero issue (no paywall exists). Open P0-class issue: the
drain_pool recurrence (§3). Tripwire: TRIPPED (§3).

## 2. Gate decision + remediation log

**Gate: BLOCKED. No tag. No merge.**

- A4 requires composite ≥80 AND no automatic-zero AND no open P0 AND a clean
  tripwire. Composite is 69.5; the tripwire tripped twice post-merge.
- Remediation, attempt 1 (in-scope, policy-compliant): the one un-remediated
  red main run (`43405043941`, drain in `orphan_work_file...`, never rerun)
  received its single allowed failed-job rerun. Result: red AGAIN
  (`plaintext_database_migrates_in_place`, same `1 remain / N idle` drain
  signature). Bookkeeping completed per flake policy: issue #43 +
  FLAKES.md recurrence row (this batch).
- Remediation, attempt 2: none available. Candidate code changes are all
  excluded: quarantining persist tests is forbidden (`docs/CI.md`);
  weakening assertions or the 30s bound is forbidden (evidence + standing
  orders); a sqlx upgrade needs architect authorization and could not be
  verified against a load-dependent race; any other edit would be
  unverifiable speculation, explicitly forbidden. The failure protocol
  permits at most 2 attempts — attempt 1 exhausted the legitimate set.
- Therefore: P8 fails, gate BLOCKED, no `v0.5.0-alpha.1` tag, no merge.
  This branch (`chore/batch-1-audit-prompt-010`) is pushed for the record
  only.

## 3. Tripwire scan results

Method: `gh run list --workflow=ci --branch main` (all runs at/after the
002 merge; prompt cites commit `92d4241`, which does not exist in this repo —
`git cat-file -t 92d4241` fails — so the window is anchored at the actual
002 merge commit `6e3118f`, PR #29; deviation recorded in §7), then
per-failure log inspection for the `connection pool did not drain` signature.

| Run                                      | Date (UTC) | Failing test                              | Signature                     | Disposition                                     |
| ---------------------------------------- | ---------- | ----------------------------------------- | ----------------------------- | ----------------------------------------------- |
| 43413993015 att.1 (main, #26 merge)      | 2026-11-13 | `plaintext_database_migrates_in_place`    | 1 remain; 2 idle before close | rerun green; issue #38; FLAKES row (Prompt 008) |
| 43405043941 att.1 (main, docs #33 merge) | 2026-11-13 | `orphan_work_file_is_adopted_not_deleted` | 1 remain; 3 idle before close | left red (no rerun until this audit)            |
| 43405043941 att.2 (this audit's rerun)   | 2026-11-13 | `plaintext_database_migrates_in_place`    | 1 remain; 3 idle before close | red again; issue #43; FLAKES row (this batch)   |

MacOS stress leg: still absent (runners never dispatch; in-code justification
retained). Recorded as the standing infra deviation, not a product failure —
same finding as Prompt 002 §7 and Prompt 005 §6. Re-add macOS when runner
capacity exists (carried WP).

## 4. Tag + CI links

- Tag `v0.5.0-alpha.1`: **NOT CREATED** (blocked gate). No binaries, no
  installer links, no release claims exist anywhere to retract.
- Main CI at audit time: latest push run `43422920531` (docs #42) green;
  the audit's own rerun documented above.

## 5. Roadmap/devlog status

- NOT updated (gated on unblock). `docs/ROADMAP.md` Batch-2 preview and
  `docs/devlog/001.md` draft remain unwritten; publishing was already gated
  on O-1 and now additionally on a green gate. No devlog published anywhere.

## 6. Batch-1 retrospective (honest)

Shipped: clean-machine bootstrap, deterministic drain contract + CI detector,
governance constitution, Apache-2.0, both launch blockers, least-privilege
CI, 4 merged Batch-5 PRs with in-scope security hardening, parity matrix
machine with CI validator + charter checks, threat register, and a full
evidence trail (reports 001–009). Not shipped: a green reliability tripwire
(the drain race survives the contract), P3 enforcement, devlog cadence,
competitor baselines (Prompt 013), and the milestone tag. The audit did its
job: it found the one thing that must block, with evidence attached.

## 7. Deviations / UNKNOWNs

- Prompt cites `92d4241` as the 002 hardening commit: ABSENT from this repo
  (`git cat-file` fails). Tripwire window anchored at the real merge
  `6e3118f` instead; sustancia unchanged (all post-merge history scanned).
- Composite weighting: equal weights (prompt specifies "weighted" without
  weights); sub-scores above let the architect re-weight — the gate fails on
  the tripwire regardless of weighting.
- UNKNOWN: whether the stuck worker is a sqlx-internal scheduling stall vs a
  leaked checkout (post-close idle>0 points at staleness, never proven);
  macOS runner capacity timeline; O-1 date.

## 8. Commit SHA + CI URLs + status

| Item              | Value                                                                                                                     |
| ----------------- | ------------------------------------------------------------------------------------------------------------------------- |
| Audit branch      | `chore/batch-1-audit-prompt-010` (pushed, UNMERGED by design)                                                             |
| Report            | `reports/010-batch-1-audit-BLOCKED.md` (this file; per failure protocol)                                                  |
| Tripwire evidence | runs `43413993015` (att.1 red / att.2 green), `43405043941` (att.1 red / att.2 red); issues #38, #43; FLAKES.md rows      |
| Local gates       | `pnpm lint/typecheck/test/build` green; `cargo fmt/clippy/test --workspace` green; 100-cycle stress pass; validator green |
| Tag               | NONE — deliberately not created                                                                                           |
| Status            | **BLOCKED — STOP**                                                                                                        |
