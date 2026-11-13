# Report 009 — Parity matrix machine (Prompt 009)

- Date: 2026-11-13 (UTC)
- Agent: OpenCode (sole engineer)
- Batch: 1 / Prompt: 009 (infrastructure build; zero competitor research)
- Read order followed: `AGENTS.md` → `VISION.md` → `docs/DECISIONS.md` →
  `reports/001-*` … `008-*` → `docs/CI.md` (required set read, never hardcoded)
- Branch: `feat/parity-machine-prompt-009` (from post-merge `main`
  `7b3fb35`)

## 1. Category/row counts + priority distribution

150 rows across all 14 categories (coding 14, terminal 8, browser 13,
computer-use 11, orchestration 12, memory 10, skills 9, scheduling 9,
channels 12, artifacts 9, voice 8, enterprise 10, extensibility 11,
platform 14). Priorities per D5 (wedge = P0): P0 = coding/browser/
evidence/policy/desktop core (31 rows); everything shipped non-wedge = P1;
unshipped Batch-5+ roadmap = P2 (voice all-P2, marketplace/mobile/IDE
absent-P2, signed releases the lone P0 partial).

## 2. Talos_current stats (honest baseline)

- native 120 · partial 7 · absent 22 · unknown 1. P0 coverage: 30/31 native.
- The single non-native P0 is `platform-signed-releases` (partial):
  offline signing tested, no signed release published yet.
- The single `unknown` is `platform-a11y` (WCAG 2.1 AA audit never performed).
- All partial rows carry a `note` with the documented gap (validator-enforced).
- Every native/partial row cites existing files/test ids on post-merge
  `main`; the validator checks each citation exists (and each named test
  string occurs in its file).

## 3. Validator + charter-check design and CI wiring

- `scripts/parity/validate-matrix.mjs` (zero deps): schema (row shape,
  14 categories, enums, exact 8 competitor keys, unique ids), evidence
  existence (path must exist; `path::symbol` must contain the symbol;
  build output never counts), deterministic SUMMARY.md regeneration
  (byte-compare; drift ⇒ FAIL), charter checks (LICENSE Apache-2.0 text +
  every manifest `license`; zero `security@talos.dev` and zero
  non-`updates.talos.dev` talos.dev in live policy surfaces; reports
  001–008 present; all competitor cells `unknown`; FLAKES.md exists with
  no overdue OPEN rows). Exit 1 on any finding.
- `--write` regenerates `docs/parity/SUMMARY.md` (never hand-edited;
  `.prettierignore`d since prettier reflows generated tables — the validator
  owns its bytes).
- CI: new `parity` job (ubuntu, `node --check`, least-privilege
  `contents: read`); documented as convention-required in `docs/CI.md`.
  Swarm: new 11th `ParityAuditAgent` runs the same validator (critical on
  failure ⇒ fails the `audit` job too). Count references updated
  (10→11; `tools/audit/README.md` table + description, runner comment,
  `ci.yml` step name, `README.md` ×3, audit package description).
- Negative-tested: mutating a competitor cell / dropping evidence / stating
  partial without a note each fail `--check` independently (an early
  fail-fast version masked the competitor check — fixed and re-verified).

## 4. Threat register summary

`docs/THREAT_REGISTER.md`: T1 OpenClaw (velocity + backing; counter:
verified execution + unification; cadence: quarterly/advisory-driven),
T2 Hermes (learning loop + portal monetization; counter: evidence ledger +
open GEPA + P10 charter; cadence: quarterly with D6 review), T3 DeepSeek
Harness (D11; NO official desktop client as of last verification —
re-verify before launch; counter: ship the desktop wedge first), T4
platform-owner bundling (counter: local-first sovereignty + open source).
Each has threat, evidence date (2026-11-12/13), counter-move, review
cadence + a cadence log table.

## 5. Deviations / UNKNOWNs

- Validator initially used strict `startsWith("Apache License")` (real file
  indents the header) and naive `talos.dev` matching (flagged the designed
  `updates.talos.dev` placeholder the Prompt-004 sweep deliberately kept).
  Both corrected against repo reality; the 004 classification stands.
- ~19 evidence citations from memory failed validation on first run (wrong
  file or test name); every one fixed by grepping the actual tree. Lesson
  recorded: the validator earns its keep — never assert evidence by memory.
- UNKNOWNs carried forward: WCAG audit status (platform-a11y row);
  macOS/Linux device testing; DeepSeek desktop re-verification (D11/O-1).
- P0 definition follows D5 literally (coding+browser+evidence+enablers);
  re-tiering needs an architect decision, not a silent edit.

## 6. Commit SHA + CI URLs + status

| Item                 | Value                                                                                                  |
| -------------------- | ------------------------------------------------------------------------------------------------------ |
| Commit               | `feat(audit): parity matrix machine, charter checks, threat register [Prompt-009]` — SHA TBD at commit |
| Branch               | `feat/parity-machine-prompt-009` (on `main` `7b3fb35`)                                                 |
| CI (PR)              | PR not yet opened — URL TBD; required set per `docs/CI.md` incl. new `parity` job                      |
| CI (main post-merge) | TBD after merge                                                                                        |
| prettier             | `--write` on all touched files, `--check` clean (SUMMARY.md via `.prettierignore`)                     |
| Live research        | None performed — zero competitor cells filled (all `unknown`)                                          |
