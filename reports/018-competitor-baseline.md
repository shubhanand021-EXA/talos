# Report 018 — Verified competitor baseline seed import, provenance, T1/T3 reset, D11 nuance (Prompt 018)

- Date: 2026-11-14 (UTC)
- Agent: OpenCode (sole engineer)
- Batch: 2 / Prompt: 018 (parity baseline; NO web research performed)
- Read order followed: `AGENTS.md` → `VISION.md` → `docs/DECISIONS.md` (D11) →
  `reports/009-parity-machine.md` → `docs/parity/CAPABILITY_MATRIX.json` →
  `docs/THREAT_REGISTER.md` → `docs/CI.md` (+ latest `reports/017` branch state)
- Branch: `feat/competitor-baseline-prompt-018` (from `main` `a49689e`)

## 0. Scope note (009/011/013/014/016 precedent)

685 line-changes across 6 files — over the ~600 target. The bulk IS the
mandated import (145 seeded cells + 145-entry provenance map + sources
legend in `CAPABILITY_MATRIX.json`); validator, SUMMARY, threat
register, and D11 nuance are its inseparable companions. Splitting would
half-import the baseline the prompt exists to ship, so the overage is
documented, not split. No web research was performed; no cell invented.

## 1. Provenance schema delta

`validate-matrix.mjs`: new `matrix.sources` legend (S1/S2 with
description + verified_date + confidence) and `matrix.provenance` map
(row → competitor → source key). New checks: seeded cells (value ≠
`unknown`) REQUIRE a known source whose legend carries date+confidence
(`provenance-required`, `provenance-source`); unknown cells MUST carry
no provenance (`provenance-unseeded`); provenance keys/rows must be
sanctioned (`provenance-key`, `provenance-row`, `sources-missing`,
`sources-entry`, `provenance-shape`). The old
all-unknown rule is replaced (not weakened — every unseeded cell is
still asserted `unknown`, now with a provenance-absence proof).
Confidence: S1 = high (inference documented: live verification
outranks second-hand reports, whose medium the architect stated);
S2 date kept exactly as given (`2026-11`, no day invented).
SUMMARY footer regenerated deterministically (Prompt 013 → 018 wording).

## 2. Import table (seeded counts per competitor)

23 of 25 seed rows imported verbatim onto existing matrix rows (cell =
S-value mapped N→native/P→partial/A→absent/U→unknown; source attached):

| Seed row                      | Matrix row                   | Note                                                          |
| ----------------------------- | ---------------------------- | ------------------------------------------------------------- |
| official-native-desktop-gui   | platform-tauri-shell         | exact                                                         |
| persistent-background-daemon  | orchestration-gateway-daemon | exact                                                         |
| telegram-discord-channels     | channels-telegram-poll       | representative; Discord rows stay unknown                     |
| natural-language-scheduler    | scheduling-heartbeat-parse   | exact                                                         |
| byoc-remote-runners           | enterprise-team-workspaces   | approximate (closest shared-remote row; no runner row exists) |
| hash-chained-evidence-ledger  | artifacts-hash-chain         | exact                                                         |
| default-deny-policy-broker    | computer-safety-envelope     | approximate (only "default-deny" row; scoped to computer-use) |
| os-process-sandbox            | terminal-exec-allowlist      | approximate (closest subprocess-constraint row)               |
| encrypted-at-rest-evidence    | memory-encrypted-backups     | exact (backups are the at-rest evidence store)                |
| mcp-client-server             | extensibility-mcp-client     | representative; server row stays unknown                      |
| instruction-file-compat       | skills-instruction-files     | broadest format list                                          |
| skill-learning-verified-tasks | skills-gepa-distill          | exact                                                         |
| local-embeddings-memory       | memory-local-embeddings      | exact                                                         |
| byok-multi-provider-gateway   | enterprise-keychain-byok     | approximate (BYOK-key half; gateway routing unmodeled)        |
| cost-budgets-governor         | orchestration-budget-caps    | exact                                                         |
| headless-cli                  | terminal-headless-cli        | exact                                                         |
| signed-auto-update            | platform-auto-update         | update-pipeline half; signing covered by charter checks       |
| multi-agent-delegation        | orchestration-supervisor     | exact                                                         |
| browser-automation            | browser-click-type           | representative action row                                     |
| computer-use                  | computer-actions             | exact                                                         |
| hitl-approvals                | orchestration-hitl           | exact                                                         |
| context-compaction            | memory-budget-context        | exact                                                         |
| offline-first-local           | platform-offline-local       | exact concept match                                           |

Seeded cells: 145 total — openclaw 23, hermes 4, open_interpreter 23,
opencode 23, claude_code 23, codex 23, abacus 10, deepseek_harness 16
(counted from the file; matches the prompt letter-strings row by row).
Unseeded cells (all untouched rows): `unknown` with no provenance,
enforced.

## 3. Threat register reset + D11 nuance

- T3: desktop SHIPPED (macOS AS + Windows, Electron, MIT, dsh-0.2.2
  merged 2026-11-13); "no desktop client" window CLOSED; counter-move
  re-positioned to verified execution + evidence ledger + default-deny
  governance + multi-provider BYOK neutrality + enterprise audit +
  Tauri-light performance; evidence 2026-11-14; cadence weekly.
- T1: native Windows Hub + Tauri desktop verified 2026-11-14
  (docs.openclaw.ai/platforms/windows); counter-move adds governance moat;
  CVE-2026-25253/33579 evidence retained. T2/T4 untouched.
- Cadence log row added.
- `docs/DECISIONS.md`: D11 block preserved byte-for-byte; nuance
  appended AFTER D12 under its own heading (status locked, premise
  overturned 2026-11-14, wedge re-positioned, T1/T3 pointers).
  No other decision touched.

## 4. Validator/charter results

- `node scripts/parity/validate-matrix.mjs --write` + `--check` → exit 0
  (`parity validation ok: 150 rows, charter checks green`).
- Negative tests (scratch copies, matrix restored byte-identical after):
  seeded-without-provenance → FAIL `provenance-required`; unknown-with-
  provenance → FAIL `provenance-unseeded`; unknown source → FAIL
  `provenance-source`.
- Parity agent docstring updated (no logic change; its tests mock exec).
- `pnpm audit:swarm` → exit 0; `ParityAuditAgent (critical 0,
warnings 0)` on the real seeded matrix.

## 5. Flagged contradictions (architect review, NOT silent overrides)

Repo-evidence sweep (deepseek/openclaw greps over repo): no citable
repo evidence about competitors' actual capabilities — nothing to
contradict the seed, so NO UNKNOWN-for-review overrides. Two seed
rows could NOT be placed without overwriting or inventing (flagged,
cells stay `unknown`, validator holds):

1. `local-model-ollama` — only textual home is
   `platform-offline-local`, already taken by the exact-match
   `offline-first-local`; single-vendor note in the prompt confirms the
   row matters. Needs architect call: split the row or reassign.
2. `open-source-license` — NO matrix row denotes licensing (charter
   checks cover LICENSE, but no capability row exists). Needs
   architect call: add a row (with Talos side + priority) or drop.

Approximate mappings (§2 table, 7 rows) are documented, not hidden;
architect may re-point them on review.

## 6. Commit SHA + CI URLs + status

| Item   | Value                                                                                                           |
| ------ | --------------------------------------------------------------------------------------------------------------- |
| Commit | `feat(audit): import verified competitor baseline seed; reset T1/T3 threat register [Prompt-018]` (SHA on push) |
| Branch | `feat/competitor-baseline-prompt-018` (from `main` `a49689e`)                                                   |
| PR     | URL + CI run URL + status before merge                                                                          |
| CodeQL | Unchanged paths only for JS alerts (validator is new logic, no sinks); post-merge API re-pull recorded here     |

### Gate evidence (exit codes; durations where measured)

- `node scripts/parity/validate-matrix.mjs --check` → exit 0, 150 rows.
- Negative enforcement probes → 3× exit 1 with the expected check names.
- `pnpm lint/typecheck/test/build` → exit 0 (contracts 114, audit 34,
  browser-use 58, ui 42 files/202 tests, first-try green).
- `pnpm audit` → 1 moderate pre-existing (minimatch/eslint, lockfile
  byte-identical), critical-clean.
- `pnpm audit:swarm` → exit 0, 0 criticals, ParityAgent 0/0 on seeded matrix.
- `cargo fmt --check` (crates + src-tauri) → exit 0;
  `cargo clippy --workspace --all-targets -- -D warnings` → exit 0;
  `cargo test --workspace` → exit 0, all suites green.
- No `.rs` changes (`git diff --name-only -- *.rs` empty).

Final line: commit SHA + CI run URL + green status recorded on merge.
