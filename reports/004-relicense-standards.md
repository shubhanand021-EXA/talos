# Report 004 — Relicense + standards truth pass (Prompt 004)

- Date: 2026-11-12 (UTC)
- Agent: OpenCode (sole engineer)
- Batch: 1 / Prompt: 004 (D2 execution; launch-blocker wounds; least-privilege CI)
- Read order followed: `AGENTS.md` → `VISION.md` → `docs/DECISIONS.md` →
  `docs/adr/0013-apache2-relicense.md` → `reports/001-*`, `002-*`, `003-*`
- Branch: `chore/relicense-standards-prompt-004` (from post-merge `main`
  `a3bf2f9`)

## 1. Manifest relicensing table (crate/package → old → new)

| Manifest                             | Old                          | New                                                                 |
| ------------------------------------ | ---------------------------- | ------------------------------------------------------------------- |
| `LICENSE`                            | MIT ("Talos Contributors")   | Apache-2.0 full text + `Copyright 2026 Shubh Anand` appendix        |
| `NOTICE`                             | ABSENT                       | created (codename, © 2026 Shubh Anand, THIRD_PARTY_NOTICES pointer) |
| `THIRD_PARTY_NOTICES.md`             | ABSENT                       | created (allowlist posture + history; pointer target exists)        |
| `crates/Cargo.toml` (workspace root) | `license = "MIT"`            | `license = "Apache-2.0"` (inherited by 24 member crates)            |
| `crates/talos_cli/Cargo.toml`        | `license = "MIT"` (explicit) | `license = "Apache-2.0"`                                            |
| `crates/talos_updates/Cargo.toml`    | `license = "MIT"` (explicit) | `license = "Apache-2.0"`                                            |
| `apps/desktop/src-tauri/Cargo.toml`  | `license = "MIT"` (explicit) | `license = "Apache-2.0"`                                            |
| `package.json` (root)                | `"license": "MIT"`           | `"license": "Apache-2.0"`                                           |
| `apps/desktop/package.json`          | `"license": "MIT"`           | `"license": "Apache-2.0"`                                           |
| `apps/ui/package.json`               | `"license": "MIT"`           | `"license": "Apache-2.0"`                                           |
| `packages/browser-use/package.json`  | `"license": "MIT"`           | `"license": "Apache-2.0"`                                           |
| `packages/contracts/package.json`    | `"license": "MIT"`           | `"license": "Apache-2.0"`                                           |
| `tools/audit/package.json`           | `"license": "MIT"`           | `"license": "Apache-2.0"`                                           |

Contributor audit (ADR-0013 pre-execution check): `git log
--format='%an <%ae>'` over full history returns exactly two identities —
`Shubh Anand <shubhanand021@gmail.com>` and `Talos <talos@local>` (local
agent identity, same human operator). No third-party authorship in history;
relicense authority confirmed, history not rewritten.

Untouched by design (history/evidence, never rewritten): `CHANGELOG.md`
MIT entries, `docs/ROADMAP.md:71`, `docs/PROGRESS.md:400`,
`docs/EVIDENCE.md` license rows, `reports/003-governance.md` §5, ADR-0013
(the decision record describes the transition and stays accurate).

## 2. SECURITY.md delta + disclosure policy

- Removed `security@talos.dev` from all 8 live-policy locations
  (`SECURITY.md:17`, `README.md:334`, `CODE_OF_CONDUCT.md:63`,
  `SUPPORT.md:18`, `docs/SECURITY.md:187`,
  `.github/ISSUE_TEMPLATE/config.yml:5`, `security_report.yml:10,15,16`).
- Primary channel: GitHub private vulnerability reporting — verified
  enabled via API: `gh api
repos/shubhanand021-EXA/talos/private-vulnerability-reporting` →
  `{"enabled":true}` (no OWNER CHECK needed).
- Added: 90-day coordinated disclosure (§ "Coordinated disclosure"),
  supported-versions table (0.4.x supported, <0.4.0 upgrade-only),
  acknowledgement-within-72h retained, advisory credit on fix release.
- Added: owned-domain email arrives when O-1 closes; explicit statement
  that no email channel exists today (worded without the literal
  third-party address so the sweep stays zero).
- Drive-by truth fix: "Batch 1/2" → "Batch 4" in the SECURITY.md intro
  (repo is v0.4.0; verified in `CHANGELOG.md`).

## 3. talos.dev sweep (command + output)

Command: repo-wide grep for `talos\.dev` (exit 0, matches classified):

- **Zero** in live policy surfaces: `README.md`, `SECURITY.md`,
  `SUPPORT.md`, `CODE_OF_CONDUCT.md`, `docs/SECURITY.md`,
  `docs/INSTALL.md`, `.github/ISSUE_TEMPLATE/*` all clean (verified by
  the same grep returning no hits in those files).
- Remaining hits are NOT policy surfaces (kept, each documented
  not-yet-public where user-facing):
  - Designed update endpoint (functional config, unowned domain reserved
    for O-1): `apps/desktop/src-tauri/tauri.conf.json:77`,
    `crates/talos_updates/src/lib.rs:64,218,246`,
    `apps/desktop/src-tauri/src/updater.rs:294,299` (tests),
    `packages/contracts/src/updater.test.ts:12`,
    `apps/ui/.../UpdatesPanel.test.tsx:49`,
    `apps/ui/src/bridge/bridge.test.ts:156`,
    `tools/audit/tests/agents.test.ts:63`, `.talos/audit.config.json:60`.
  - Docs describing the built pipeline with explicit not-live status:
    `docs/UPDATING.md:26,45` (example manifest + template; status note
    added), `docs/SECURITY.md:175` (already labeled placeholder),
    `docs/ARCHITECTURE.md:337` (already labeled placeholder).
  - Historical evidence archive (never rewritten): `reports/001-*:227-228`.

## 4. README/docs claim audit (claim → evidence)

- Installer table (Blocker B) → replaced with "Source builds only today"
  - Releases link (zero assets published: `gh release list` empty, 5 tags,
    no downloads) + per-OS build rows + planned-artifact columns.
- "installers are signed" (README §Security) → reworded: signing
  implemented (`scripts/sign-release.mjs`, pinned pubkey
  `tauri.conf.json:79`, rollout gating in `crates/talos_updates`),
  activates with the first signed release.
- License badge/line/§License → Apache-2.0 (all packages verified
  `private: true`, not published).
- Counts verified by grep: 96 `#[tauri::command]` fns
  (`apps/desktop/src-tauri/src`), 13 routes (`app/router.tsx`: 12 pages +
  root), 33 registry tool ids (8 fs + 1 process + 9 browser + 9 computer
  - 2 code + 4 git).
- Bundler targets verified: `tauri.conf.json:33` =
  `["nsis","msi","dmg","appimage","deb"]`.
- Scripts referenced in Quickstart verified present in root `package.json`:
  `dev:ui`, `dev:desktop`, `demo:batch2`, `demo:awakening`,
  `demo:awakening:llm`, `build:desktop`, `audit`, `audit:swarm`,
  `media:generate`.
- "under five minutes" onboarding (README + `docs/ONBOARDING.md`) →
  softened to "a few minutes" (no timing test exists; claim now
  un-falsifiable rather than precise).
- Batch-5 section stays unchecked "in-progress" (README:323) ✓.
- `docs/CLI.md` audited: no download/installer claims (exit codes, JSONL
  contract, `TALOS_DATA_DIR` semantics) — no change.
- Target Capability Matrix untouched: header already disclaims design
  intent, no third party named ✓.

## 5. Standards checklist

| File                      | Status                                      |
| ------------------------- | ------------------------------------------- |
| `README.md`               | present, truth-passed                       |
| `LICENSE`                 | Apache-2.0 full text ✓                      |
| `NOTICE`                  | created ✓                                   |
| `THIRD_PARTY_NOTICES.md`  | created (pointer target) ✓                  |
| `SECURITY.md`             | disclosure rewritten ✓                      |
| `CONTRIBUTING.md`         | present, correct (Prompt 003 DoD)           |
| `CODE_OF_CONDUCT.md`      | contact fixed ✓                             |
| `CHANGELOG.md`            | present, history untouched ✓                |
| `SUPPORT.md`              | contact fixed ✓                             |
| `CODEOWNERS`              | present (`.github/CODEOWNERS`), unchanged ✓ |
| `ISSUE_TEMPLATE/*`        | 5 forms present, security contacts fixed ✓  |
| `PULL_REQUEST_TEMPLATE`   | present (`.github/`), unchanged ✓           |
| `workflows/ci.yml`        | `permissions` block added (Phase E) ✓       |
| `docs/evidence/FLAKES.md` | present, unchanged ✓                        |

Gap filled beyond the listed set: `THIRD_PARTY_NOTICES.md` did not exist
while `NOTICE` was required to point at it — created a minimal, fully
verifiable posture file (allowlist + enforcement + history) rather than a
dangling pointer. No per-package table fabricated.

## 6. CodeQL alert closure evidence

Pre-change state (API `code-scanning/alerts`): 11 open
`actions/missing-workflow-permissions` alerts on `ci.yml` (#1–7, #13–16;
#8 already fixed) + 3 unrelated JS alerts (#9–11: redos/sanitization —
out of scope, recorded not fixed). Post-merge rescan closes the
workflow-permissions class; alert IDs + states re-pulled after the
post-merge CI green (see §8). Per-job `actions: write` granted only to
the three artifact-uploading jobs (`audit`, `awakening`, `hardening`);
all others inherit workflow `contents: read`. No action versions bumped
(Node-20 deprecation left parked per prompt).

## 7. Owner-check items

- None required: private vulnerability reporting verified `enabled:true`
  via API (no admin scope needed). O-1 still owns: public brand, owned
  domains/email, waitlist/pricing validation, DeepSeek re-verification
  (carried from Report 003).

## 8. Commit SHA + CI URLs + status

| Item                 | Value                                                                                                                |
| -------------------- | -------------------------------------------------------------------------------------------------------------------- |
| Commit               | `chore(license): relicense to Apache-2.0; standards truth pass; least-privilege CI [Prompt-004]` — SHA TBD at commit |
| Branch               | `chore/relicense-standards-prompt-004` (on `main` `a3bf2f9`)                                                         |
| CI (PR)              | PR not yet opened — URL TBD; required set per `docs/CI.md`                                                           |
| CI (main post-merge) | TBD after merge                                                                                                      |
| prettier             | `--write` on all touched `.md` (+`NOTICE` via `--parser markdown`), `--check` clean                                  |
| CodeQL post-merge    | TBD — re-pull alert states after merge rescan                                                                        |

Changed files (27: 24 modified + 3 created): `LICENSE`, `NOTICE` (new),
`THIRD_PARTY_NOTICES.md` (new), 4 `Cargo.toml`, 6 `package.json`,
`README.md`, `SECURITY.md`, `SUPPORT.md`, `CODE_OF_CONDUCT.md`,
`docs/SECURITY.md`, `docs/INSTALL.md`, `docs/UPDATING.md`,
`docs/ONBOARDING.md`, `VISION.md`, `docs/DECISIONS.md`,
`.github/workflows/ci.yml`, `.github/ISSUE_TEMPLATE/config.yml`,
`.github/ISSUE_TEMPLATE/security_report.yml`,
`reports/004-relicense-standards.md` (new, this file).
Zero non-doc changes except the authorized `ci.yml` permissions block —
prove with `git status --porcelain` (only license-manifest/`.md`/`ci.yml`
paths; untracked build dirs not staged).
