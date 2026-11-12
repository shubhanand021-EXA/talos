# Report 001 — Clean-Machine Bootstrap & Truth Baseline

- Date: 2026-11-12 (UTC)
- Agent: OpenCode (Muse Spark 1.3 Contributor)
- Batch: 1 / Prompt: 001 (bootstrap + verification; zero product-logic changes)
- Read order followed: AGENTS.md → VISION.md (ABSENT, used README.md interim) →
  docs/DECISIONS.md (ABSENT, used docs/adr/0001–0012 interim) →
  latest reports entry (ABSENT at repo root, used docs/reports/audit-latest.md +
  docs/evidence/batch-4-final-report.md interim)

## 1. Environment (OS, arch, shell, every tool + exact version)

- OS: Microsoft Windows 11 Home Single Language, OsVersion 10.0.26300,
  WindowsBuildLabEx 26100.1.amd64fre.ge_release.240331-1435, WindowsVersion 2009
- Arch: AMD64, 64-bit (Is64BitOperatingSystem True)
- Shell: Windows PowerShell 5.1.26100.9711 (Desktop edition); winget v1.29.380
- Workspace `D:\Project` was EMPTY (0 entries); machine had NONE of
  git/gh/node/pnpm/cargo/rustc/just installed — clean machine confirmed.
- git version 2.56.0.windows.4 (winget Git.Git 2.56.0.4, newly installed)
- gh version 2.103.3 (2026-10-26) (winget GitHub.cli 2.103.3, newly installed);
  `gh auth status`: logged in as `shubhanand021-EXA`, scopes
  gist/read:org/repo/workflow — requirement (repo + workflow) satisfied.
- node v26.13.1 (winget OpenJS.NodeJS.LTS 26.13.1, 2026-11-10, newly installed);
  npm 11.20.0
- pnpm 10.34.7 (via `npm install -g pnpm@10.34.7`; corepack is ABSENT from the
  Node 26.13.1 distribution — see §9)
- rustup 1.29.3 (winget Rustlang.Rustup 1.29.3, newly installed)
- rustc 1.99.1 (58cf8f9c3 2026-10-13); cargo 1.99.1 (5f94df478 2026-08-27);
  rustfmt 1.10.0-stable (58cf8f9c35 2026-10-13); clippy 0.1.99 (58cf8f9c35);
  default toolchain set to 1.99.1-x86_64-pc-windows-msvc
- just 1.59.0 (winget Casey.Just 1.59.0, newly installed)
- NSIS 3.13 / makensis (winget NSIS.NSIS 3.13, newly installed)
- WebView2 Evergreen Runtime 157.0.4341.46 — PRE-EXISTING (no install needed)
- Windows SDK 10.0.26100.7705 (winget Microsoft.WindowsSDK.10.0.26100, newly
  installed; `C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0` present;
  prior tree held only a stub `UnionMetadata` dir)
- VS Build Tools 2022 17.14.45 + VCTools workload, MSVC 14.44.35207
  (newly installed; `cl.exe`/`link.exe` verified at
  `...\VC\Tools\MSVC\14.44.35207\bin\Hostx64\x64\`). Prior: only an empty
  `2022/` skeleton + .NET SDK 8.0.426 + VCRedists; no `link.exe` anywhere.

## 2. Pin Reconciliation (repo pin file → value → installed value → match?)

| Repo pin file | Value | Installed | Match? |
|---|---|---|---|
| `.nvmrc` | `26` (major only) | node v26.13.1 (current LTS) | YES (major; exact 26.13.1 recorded) |
| `package.json` → `packageManager` | `pnpm@10.34.7` | pnpm 10.34.7 | YES exact |
| README badge → Rust (points at `crates/Cargo.toml`) | `1.99.1` | rustc/cargo 1.99.1 | YES exact |
| `rust-toolchain.toml` | ABSENT (glob no match) | created, pins `1.99.1` + rustfmt/clippy | YES now (was drift-risk) |
| `justfile` / README Quickstart | `just >= 1.30` | just 1.59.0 | YES |
| README Quickstart | `git >= 2.40` | git 2.56.0 | YES |
| `crates/Cargo.toml` | sqlx `0.8`, no `rust-version` field | sqlx 0.8.6 resolved | recorded (see §6) |

Deviations: (a) corepack shim does not exist in Node 26.13.1
(`C:\Program Files\nodejs` ships no corepack) — pnpm pinned via
`npm install -g pnpm@10.34.7` instead of
`corepack prepare pnpm@10.34.7 --activate`; (b) CI `rust-core` uses
`dtolnay/rust-toolchain@stable` (unpinned), which currently resolves to the
same 1.99.1 — badge and local pin agree today, CI float noted as risk (§9).

## 3. Commands Executed (command → exit code → duration)

Phase A (bootstrap):
- `winget install --id Git.Git -e` → 0 (git 2.56.0.windows.4)
- `winget install --id GitHub.cli -e` → 0 (gh 2.103.3)
- `gh auth status` → 0 (shubhanand021-EXA, repo+workflow scopes)
- `gh repo clone shubhanand021-EXA/talos` → 0
- `winget install --id OpenJS.NodeJS.LTS -e --version 26.13.1` → 0
- `npm install -g pnpm@10.34.7` → 0 (12s)
- `winget install --id Rustlang.Rustup -e` → 0 (rustup 1.29.3)
- `rustup toolchain install 1.99.1; rustup default 1.99.1; rustup component add rustfmt clippy` → 0
- `winget install --id Casey.Just -e` → 0 (just 1.59.0)
- `winget install --id NSIS.NSIS -e` → 0 (3.13)
- `winget install --id Microsoft.WindowsSDK.10.0.26100 -e` → 0 (10.0.26100.7705)
- `winget install --id Microsoft.VisualStudio.2022.BuildTools -e --override "--wait --quiet --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"` → 0
- `pnpm install --frozen-lockfile` → 0, 34s (308 packages; lockfile up to date)
- `cargo fetch` (in `crates/`) → 0, 253s

Phase B (gates — full detail in §4):
- `pnpm typecheck` (pre-build) → 2, 16s (missing `dist/`, see §9)
- `pnpm build` → 0, 15s
- `pnpm typecheck` (post-build) → 0, 18s
- `pnpm lint` → 1, 12s (318 prettier warnings; Windows CRLF artifact, §9)
- `pnpm test` → 0, 83s
- `cargo fmt --all -- --check` (in `crates/`) → 0, 7s
- `cargo clippy --workspace --all-targets -- -D warnings` (pre-MSVC) → 101, 20s (`link.exe` not found)
- `cargo clippy --workspace --all-targets -- -D warnings` (post-MSVC) → 0, 215s
- `cargo test --workspace` → 101, 272s (535 passed, 1 failed: vector_store timing, §4)
- `cargo test -p vector_store --lib search_is_fast_for_thousands` (isolated, debug) → 101, 78s (still fails)
- `cargo test -p vector_store --lib --release search_is_fast_for_thousands` → 0, 213s (env-only profile change)
- `pnpm audit:swarm` (pre-MSVC) → 1, 79s (2 toolchain-caused criticals)
- `pnpm audit:swarm` (post-MSVC) → 0, 63s (WARN 9/10 clean, 0 criticals)
- `pnpm audit --audit-level high` → 0, 1s (1 moderate, pre-existing)

Phase B (inventory): `git rev-parse HEAD`, `git log --oneline -20`,
`git rev-list --count HEAD`, `git branch -a`, `git tag -l`, `gh pr list`,
`gh pr view 24/25/26/27`, `gh pr checks 24/25/26/27`, `gh run list --limit 20` —
all exit 0 (values in §5).

## 4. Quality Gate Results (suite → passed/failed/skipped counts)

- `pnpm build` → PASS, exit 0 (contracts ESM+DTS, browser-use ESM+DTS, ui vite 2.14s)
- `pnpm typecheck` → PASS (post-build), exit 0; initial pre-build run FAILED
  exit 2 with ~90 `TS2307: Cannot find module '@talos/contracts'` errors because
  `packages/contracts/dist/` does not exist on a fresh clone. CI builds workspace
  types BEFORE typecheck (ci.yml typescript job) — ordering artifact, not a bug.
- `pnpm lint` → FAIL, exit 1: `prettier --check .` warns on 318 files.
  Root cause is environmental: repo mandates `end_of_line = lf` (.editorconfig)
  but ships no `.gitattributes`, and Git-for-Windows checked out CRLF
  (`core.autocrlf=true`). CI runs on ubuntu-latest (LF) and is unaffected.
  No product code touched.
- `pnpm test` → PASS, exit 0: root `node --test tools/` 35 pass; contracts
  vitest 20 files / 88 tests pass; audit package 29 pass; browser-use 8 files /
  46 pass; ui 32 files / 161 pass. (JS total: 359 assertions green.)
- `cargo fmt --all -- --check` → PASS, exit 0.
- `cargo clippy --workspace --all-targets -- -D warnings` → PASS, exit 0
  (post-MSVC; 215s). Pre-MSVC run failed exit 101 (`link.exe` not found) —
  environment finding, resolved by A7 Build Tools install.
- `cargo test --workspace` (debug) → 535 passed / 1 failed / 0 ignored:
  agent_runtime 92, computer_use 35, context_assembler 27, context_budget 20,
  cost_tracker 11, embedding_engine 7, evidence_store 65, instruction_parser 20,
  llm_gateway 32, mcp_client 18, mcp_server 5, memory_indexer 8, policy_broker 14,
  sandbox 21, skill_extractor 13, talos_cli 12, talos_common 7, talos_secrets 6,
  talos_telemetry 10, talos_updates 9, tool_rag 23, tool_runtime 75,
  vector_store 5 (+1 fail). Zero-test binaries: awakening_demo, batch2_demo,
  hardening_demo, real_driver, policy_broker bin, talos bin.
  SOLE FAILURE: `vector_store::tests::search_is_fast_for_thousands`
  (`crates/vector_store/src/lib.rs:608`):
  `assertion failed: started.elapsed() < Duration::from_millis(100)`.
  Bounded retries per failure protocol (no product change): isolated debug rerun
  → still FAIL (78s); `--release` rerun → PASS (213s, suite 0.45s). Verdict:
  Windows-debug-build perf artifact vs a 100ms budget; release-green and all
  CI rust-core runs green. Recorded, not patched.
- `pnpm audit:swarm` → PASS (WARN), exit 0: 9/10 agents clean, 0 criticals,
  2 warnings (both pre-existing non-runtime URL constants — PrivacyAgent
  `mockApprovals.ts:35` mailto fixture, `awakening_demo/main.rs:558`
  worldtimeapi constant — same class as EVIDENCE.md documents). Pre-MSVC run
  FAILED exit 1 (FAIL 5/10, 2 criticals) purely from missing `link.exe`
  (`could not compile ... (build script)` inside Security/Reliability/
  Evidence/Telemetry agents) — environment finding, resolved by A7.
- `pnpm audit --audit-level high` → PASS, exit 0 (1 moderate, pre-existing).

## 5. Repository Truth (HEAD SHA, commits, branches, tags, open PRs + CI, crate list)

- HEAD SHA: `04c80aafd6e35e58a56c5328fdc43dd5fb79f047` (matches prompt expectation
  `04c80aa`). `git status` clean except intended governance files (§8);
  `pnpm-lock.yaml` byte-identical (`git diff` empty).
- Commit count: 68 (`git rev-list --count HEAD`). `git log --oneline -20` head:
  `04c80aa Merge PR #23`, `a27f09f`, `d38dd7e` (flake notes), `9c5824f Merge #22`,
  `aef20cd`, `d155739`, `57c189f`, `886ea6d`, `cf96749 Merge #21` … all match.
- Branches: 27 real branches (28 `git branch -a` lines incl. the `origin/HEAD`
  symbolic artifact): main; chore/hf-03-closeout; chore/hf-03-public-normalization;
  docs/flake-note-batch4; feat/b3-context-assembly, b3-live-browser, b3-llm-gateway,
  b3-supervisor-delegation, byoc-remote-runners, distribution-layer, gateway-daemon,
  gepa-skill-learning, hitl-approval, llm-dag-planning, mcp-interop,
  memory-vector-search, messaging-channels, os-sandbox, persistence-durable-state,
  real-computer-use, real-specialist-fidelity, runtime-execution-fixes,
  scheduler-heartbeat, telemetry-cost-tracking, workspace-coding;
  release/v0.3.0-awakening, release/v0.4.0.
- Tags (`git tag -l`, 4): `v0.1.0-green-baseline`, `v0.2.0`, `v0.3.0`, `v0.4.0`.
  DEVIATION: prompt said `v0.1.0`; the real first tag is `v0.1.0-green-baseline`.
- PRs: 4 OPEN (#24 gateway daemon, #25 HEARTBEAT scheduler, #26
  Telegram/Discord channels, #27 BYOC SSH/Docker runners — titles match prompt);
  23 MERGED (#1–#23, incl. #23 flake-note, #22 v0.4.0 release); 0 closed-unmerged.
  All four open PRs: state OPEN, mergeable MERGEABLE, mergeStateStatus CLEAN.
  Checks per PR: 12 pass + 1 skipping (`browser live smoke (manual only)`,
  workflow_dispatch-only) = 13 reported. DEVIATION: prompt said "13/13 passing";
  truth is 12 pass + 1 legitimately-skipped manual job on each PR.
- Recent CI (`gh run list --limit 20`): latest scheduled `main` runs
  `completed success` (43150867729, 42955336463, 42751528282, 42558703705);
  PR #27 branch had two `completed failure` ci runs (42453300087, 42449372124)
  superseded by later `completed success` (42453593945 + CodeQL 42453591692).
- Crates (`crates/Cargo.toml` members, 27): talos_common, policy_broker,
  evidence_store, agent_runtime, tool_runtime, computer_use, context_assembler,
  workspace_manager, embedding_engine, vector_store, memory_indexer,
  talos_telemetry, cost_tracker, awakening_demo, sandbox, mcp_client, mcp_server,
  instruction_parser, skill_extractor, talos_cli, talos_updates, hardening_demo,
  batch2_demo, context_budget, tool_rag, llm_gateway, talos_secrets.
  Apps: desktop, ui. Packages: contracts, browser-use. Tools: audit, demo.
  Scripts (pre-existing): sign-release.mjs, stage-cli-binaries.mjs,
  media/generate-brand.mjs.
- Test counts per suite: §4 above (TS 359 green; Rust 535 green + 1 env-only
  timing fail, release-green).

## 6. drain_pool Evidence (verbatim quote + CI run IDs + file path)

File: `docs/EVIDENCE.md`, lines 18–39 (section
`## 2026-11-02 — Batch 4 drain_pool CI flake (pre-existing, investigated,
unresolved)`), recorded by commit `d38dd7e`, enriched by `a27f09f`. Verbatim:

```text
- Commands:
  - `cargo test --workspace -j 1` (local, Windows) → all 54 suites `ok`,
    0 failed (repeated 5x); `cargo test -p evidence_store --lib` (incl. `--test-threads 32`,
    under parallel build load) → `ok. 65 passed` every run (10+ runs).
  - CI `rust-core`: 5× `Storage("connection pool did not drain (1 connection(s) remain)")`
    after the full 200×50ms poll budget, in
    `orphan_work_file_is_adopted_not_deleted`, `encrypted_reopen_preserves_data`,
    and `plaintext_database_migrates_in_place` (3×) across runs
    41484991105, 41487649463, 41490635438 (main), 41491480674 (main, twice);
    every affected run passes on rerun with no code changes.
- Verification: reviewed sqlx 0.8.6 pool close semantics (`Pool::close`
  awaits graceful close; `size()` counts live connections; `min_connections`
  0; `optimize_on_close` disabled; `#[tokio::test]` is current-thread);
  no shared state across the isolated tempdir databases; the failing tests
  exercise pre-existing close paths untouched by Batch 4 (no new native
  deps). No local reproduction despite stress runs.
- Notes: tracked as a known issue (final report §4 item 8). Diagnostic
  landed with this entry: drain timeout errors now report `num_idle`
  alongside `size()` (no behavior change on success paths), so the next
  occurrence distinguishes a stuck checked-out connection (`idle` 0) from
  a missed idle reap (`idle` ≥ 1). Repo precedent treats such environment
  flakes as green-on-rerun.
```

- Exact error string: `Storage("connection pool did not drain (1 connection(s) remain)")`
- sqlx version: 0.8.6 (also `crates/Cargo.toml` pins sqlx `0.8`)
- CI run IDs cited: 41484991105, 41487649463, 41490635438, 41491480674
- NOT FIXED in this prompt (Prompt 002 scope). Root-cause fix already in tree:
  `drain_pool()` polls `size()==0` before rename/delete; os errors 32/33 retry
  (`docs/EVIDENCE.md:180`).

## 7. Known Launch Blockers (file:line for each)

1. `security@talos.dev` placeholder (domain owned by Sidero Labs — unresolved):
   - `SECURITY.md:17` — `Email: \`security@talos.dev\` (placeholder — …)`
   - Corroborating occurrences (same placeholder string, not separately counted
     as blockers): `README.md:334`, `CODE_OF_CONDUCT.md:63`, `SUPPORT.md:18`,
     `docs/SECURITY.md:187`, `.github/ISSUE_TEMPLATE/config.yml:5`,
     `.github/ISSUE_TEMPLATE/security_report.yml:10,15,16`
2. README installer links point to non-existent release binaries:
   - `README.md:110-112` — `> Signed installers are configured and verified
     end-to-end, but no public release channel exists yet — links below are
     placeholders until the first staged rollout.`
   - `README.md:114-118` — installer table rows `Talos_0.4.0_x64_en-US.msi`
     (or NSIS `.exe`) / `Talos_0.4.0_aarch64.dmg` / `Talos_0.4.0_amd64.AppImage`
     (or `.deb`); `docs/INSTALL.md`, `docs/CLI.md`, `docs/ONBOARDING.md`,
     `docs/UPDATING.md` describe the same not-yet-existing channel.
   - Both blockers CONFIRMED present; neither touched (Prompt 004+ scope).

## 8. Files Created (path + line count) — governance only

- `rust-toolchain.toml` — 3 lines (pins channel `1.99.1` + rustfmt/clippy)
- `scripts/bootstrap-clean-machine.ps1` — 143 lines (idempotent Windows bootstrap)
- `scripts/bootstrap-clean-machine.sh` — 127 lines (idempotent macOS/Linux bootstrap)
- `AGENTS.md` — 105 lines (standing orders per prompt §C3)
- `reports/001-bootstrap-and-baseline.md` — 293 lines (this report)

Zero product-logic files modified: `git status --short` shows only the five
paths above (4 untracked + this report); `git diff --name-only` empty for all
tracked files; `pnpm-lock.yaml` unchanged.

## 9. Deviations, UNKNOWNs, and Risks

1. First tag is `v0.1.0-green-baseline`, not `v0.1.0` as the prompt states. All
   other truth claims (68 commits, HEAD `04c80aa`, 27 branches, 23 merged PRs,
   4 open PRs) verified exact.
2. Open-PR checks are 12 pass + 1 skipping (manual `browser live smoke`),
   not "13/13 passing". All four PRs are MERGEABLE/CLEAN.
3. `pnpm lint` fails locally (exit 1, 318 prettier warnings) from Windows
   CRLF checkouts (no `.gitattributes`, `core.autocrlf=true` vs mandated LF).
   CI (ubuntu-latest) is unaffected. Left as-is: running `pnpm format` would
   dirty 318 files and violate the governance-only rule.
4. `pnpm typecheck` fails on a fresh clone until `pnpm build` generates
   `packages/*/dist/`. CI builds types first (ci.yml typescript job) — ordering
   artifact, documented in AGENTS.md §5. Passes post-build (exit 0).
5. `cargo test --workspace` (Windows debug): 1 perf-budget failure
   (`vector_store` 100ms search assertion). Release-green (0.45s suite),
   CI-green. Not patched per hard prohibition on product-logic fixes.
6. corepack is absent from Node 26.13.1; pnpm pinned via
   `npm install -g pnpm@10.34.7`. Bootstrap scripts encode this (with a
   corepack-first fallback note in §A6 comments). pnpm's self-update notice
   (12.18.0 available) was ignored to honor the pin.
7. CI `rust-core`/`desktop-check` float on `dtolnay/rust-toolchain@stable`
   while the repo badge and new `rust-toolchain.toml` pin 1.99.1 — aligned
   today (stable == 1.99.1), drift risk if stable moves. Recommend a follow-up
   prompt pin CI to the toolchain file.
8. `VISION.md`, `docs/DECISIONS.md`, and repo-root `reports/` do not exist;
   AGENTS.md read order maps them to interim equivalents (README vision,
   docs/adr/0001–0012, docs/reports + docs/evidence). Canonical files are
   future-prompt scope, not created here.
9. UNKNOWN: exact wall-clock durations of the winget installs (A2/A6/A7) were
   not timer-wrapped; versions verified post-install instead. All gate durations
   in §3 were timer-wrapped.
10. `docs/reports/audit-latest.{md,json}` + dated copies are rewritten by every
    `pnpm audit:swarm` run; both runs' outputs were captured for §4 then
    reverted (`git checkout --`) so the final diff stays governance-only.

## 10. Commit SHA + CI run URL + final status

- Commit: TBD_AFTER_PUSH (conventional: `chore(bootstrap): clean-machine toolchain, standing orders, and verified baseline [Prompt-001]`)
- CI run URL: TBD_AFTER_PUSH
- Status: TBD_AFTER_PUSH
