# Report 003 — Governance materialization (Prompt 003)

- Date: 2026-11-12 (UTC)
- Agent: OpenCode (Muse Spark 1.3 Contributor)
- Batch: 1 / Prompt: 003 (constitution materialization; zero code, zero
  dependency, zero workflow changes — docs `.md` only)
- Read order followed: `AGENTS.md` (standing orders, `main` @ `6e3118f`) →
  `reports/001-bootstrap-and-baseline.md` → `reports/002-drain-pool-fix.md`
  (`VISION.md` / `docs/DECISIONS.md` ABSENT — created by this prompt, so the
  interim mappings in `AGENTS.md` §1 applied: `README.md` vision sources,
  `docs/adr/0001–0012` decision record, `reports/001-*` now canonical)
- Branch: `docs/governance-prompt-003` (re-based onto post-merge `main`
  `6e3118f`; see §6.1 for the stale-checkout correction)

## 1. Files created / modified (paths + line counts)

| File                                     | Change                        | Lines  |
| ---------------------------------------- | ----------------------------- | ------ |
| `VISION.md`                              | created                       | 139    |
| `docs/DECISIONS.md`                      | created                       | 114    |
| `docs/adr/0013-apache2-relicense.md`     | created                       | 31     |
| `docs/adr/0014-codename-public-brand.md` | created                       | 32     |
| `docs/adr/0015-byok-auth-fence.md`       | created                       | 35     |
| `AGENTS.md`                              | §1 read-order edit            | +8/−11 |
| `CONTRIBUTING.md`                        | per-prompt DoD section append | +24    |
| `reports/TEMPLATE.md`                    | created                       | 40     |
| `reports/003-governance.md`              | created (this file)           | 121    |

Zero non-doc files changed (prove with `git status --porcelain` — §5).

## 2. Pillar table (target + verification)

| Pillar               | Target                                                                                            | Verification                                                                       |
| -------------------- | ------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| P1 Parity            | 100% P0 native at v1.0                                                                            | `docs/parity/CAPABILITY_MATRIX.json` drift blocks CI (Prompt 009)                  |
| P2 Security          | Default-deny; signed releases; 0 criticals > 72h; least-privilege tokens                          | `policy_broker` suites; `scripts/sign-release.mjs` verify; age query; scope review |
| P3 Performance       | ≤3s cold start p95; ≤100ms UI; 60fps/≤8ms GPU; ≤600MB RAM; ≤60MB installers; lite-mode            | CI perf gates per metric                                                           |
| P4 Local-first       | Offline core + local models; telemetry opt-in                                                     | Offline pass; default-off check                                                    |
| P5 AAA UX            | WCAG 2.1 AA; onboarding ≤10 min; Void Neon                                                        | Audit; timed trial; identity review                                                |
| P6 Evidence          | All side effects ledgered; replay UI; exportable reports                                          | `evidence_store` + `tool_receipt` tests; replay/export tests                       |
| P7 Open governance   | Apache-2.0; roadmap; weekly devlog; per-batch audit                                               | License gate; presence checks; `reports/`                                          |
| P8 Reliability       | ≥99.5% crash-free; flake policy per `docs/CI.md`; pool-shutdown stress each push; 30-green streak | Required checks green; `stress-shutdown`; streak                                   |
| P9 Cost transparency | Estimates ±10%; published unit costs                                                              | Estimate-vs-actual test; cost doc                                                  |
| P10 Charter          | Local free forever; paywall = AUTOMATIC ZERO; paid = cloud/teams/managed                          | Audit charter check                                                                |

## 3. Decision ledger checksum (D1–D12 present)

D1 HYBRID · D2 Apache-2.0 (execution Prompt 004) · D3 rename forced / Talos
codename / O-1 gate · D4 Tauri 2 + Rust + React 19, no crate renames ·
D5 wedge coding+browser+evidence · D6 BYOK + local only, no OAuth, quarterly
review · D7 local-free charter (=P10) · D8 pricing hypotheses, unvalidated ·
D9 governance (DoD, 10-prompt batches, ~600-line cap, merges exempt, no AI
signing/prod creds, green-tag rollback) · D10 weekly build-in-public gated O-1 ·
D11 DeepSeek Harness 8th competitor, re-verify desktop · D12 `talos-agent`
struck, fresh via O-1. All twelve recorded without alteration of substance.

## 4. ADR summaries

- **ADR 0013** MIT→Apache-2.0: decision locked here, EXECUTION in Prompt 004;
  patent-grant rationale; history never rewritten; pre-execution check —
  `LICENSE` names "Talos Contributors" (plural), contributor audit owed.
- **ADR 0014** codename vs public brand: rename drivers (5+ colliding "Talos"
  products incl. permission-kernel agent with session replay);
  transfer-not-recreate migration; O-1 gate.
- **ADR 0015** BYOK fence: BYOK + Ollama only; Apr–May 2026 Anthropic
  ban-and-reversal as volatility evidence; quarterly review; never market
  subscription reuse. Anchored on ADR 0006 (`crates/llm_gateway`,
  `crates/talos_secrets`).

## 5. Consistency checks

- `git status --porcelain`: only the nine `.md` files above (plus pre-existing
  untracked build dirs, untouched, not staged).
- Cross-links resolve: VISION ↔ DECISIONS ↔ ADR 0001–0012 + 0013/0014/0015 ↔
  `docs/CI.md` (six ruleset checks + `awakening`/`hardening`/`stress-shutdown`
  convention) ↔ `AGENTS.md` ↔ `CONTRIBUTING.md`.
- Repo anchors verified on `main`: React 19.3.1 (`apps/ui/package.json`) +
  Tauri 2 (`apps/desktop/src-tauri/Cargo.toml`) + stable `crates/` (D4 ✓);
  BYOK/Ollama/keychain + `usage_records` (ADR 0006 → D6/P9 ✓); Void Neon
  (`apps/ui/src/features/void-neon/` → P5 ✓); minisign signing
  (`scripts/sign-release.mjs`, updater pubkey; `AGENTS.md` §3 no-AI-keys
  prohibition → P2/D9 ✓); flake policy max-1-rerun + `docs/evidence/FLAKES.md`
  ≤7-day register (→ P8 ✓ exact); `stress-shutdown` 100-cycle stress (→ P8 ✓);
  public remote (ADR 0005 → D12 ✓).
- D2 note: `LICENSE` is still MIT on `main` — relicense executes in Prompt 004,
  not here. No license file touched by this batch (zero non-doc proof holds).
- `prettier --check` on all touched files: clean (after `prettier --write`).

## 6. Deviations / UNKNOWNs

1. **Stale checkout (mine, corrected).** Work began on a stale local `master`
   (`036c7ef`) whose tree predates the `main` restructure; `main` is the
   default branch (local `master` has no upstream counterpart). Branch was
   reset to post-merge `main` `6e3118f` and all files re-anchored before
   committing. Lesson recorded; no stale-tree content survives in the batch.
2. **ADR numbering format.** Prompt 003 wrote `0013–0015`; files use main's
   format `docs/adr/0013-*.md` with `# ADR 0013:` headers (main's ADRs 0001–0012
   are `0001-*.md`, not `ADR-0001-*`).
3. **D2 execution timing.** Prompt marks D2 "executed Prompt 004" — the
   relicense decision is locked in this batch (ADR 0013); execution (LICENSE +
   manifest + gate changes) belongs to Prompt 004. No `LICENSE` change here.
4. **`Cargo.toml` repository field.** Not re-verified on `main` (workspace now
   `crates/` + `apps/`); no claim carried into DECISIONS beyond the canonical
   remote from ADR 0005.

UNKNOWNs for architect: O-1 scope/owner/date; waitlist validation plan (D8);
DeepSeek Harness desktop re-verification evidence (D11); the 5+ colliding
"Talos" product list (D3/ADR-0014); Anthropic timeline sourcing beyond the
prompt (ADR-0015); "Talos Contributors" relicensing authority (Prompt 004
pre-execution); perf-gate owners/threshold encoding (P3); 30-green streak
start point (P8).

## 7. Commit SHA + CI URLs + status

| Item                 | Value                                                                                        |
| -------------------- | -------------------------------------------------------------------------------------------- |
| Commit               | `docs(governance): vision, decision ledger, ADRs 0013-0015 [Prompt-003]` — SHA TBD at commit |
| Branch               | `docs/governance-prompt-003` (on `main` `6e3118f`)                                           |
| CI (PR)              | PR not yet opened — URL TBD; required set per `docs/CI.md`                                   |
| CI (main post-merge) | TBD after merge                                                                              |
| prettier             | `npx prettier --write` on all touched files, then `--check` clean                            |
