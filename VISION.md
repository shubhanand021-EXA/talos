# Project Talos — Vision (Constitution)

Status: Accepted. Date: 2026-11-12. Owner: Shubh Anand.

Constitution documents: this file, `docs/DECISIONS.md`, `docs/adr/`.
Process authority: `docs/CI.md`. Standing orders: `AGENTS.md`.

## Mission

One local-first desktop platform replacing the fragmented agent stack.
The uninstall test: a power user can never say "competitor X has this and we don't."

## Source-of-truth hierarchy (locked)

1. Repo code + tests.
2. `VISION.md` + `docs/DECISIONS.md`.
3. `docs/parity/CAPABILITY_MATRIX.json` (Prompt 009 — not yet present).
4. Architect prompts.
5. Nothing else.

No conversation history, issue comment, or external doc outranks this order.
Where a target below differs from current repo reality, the target governs
intent and the linked reality doc governs current claims — see per-pillar
"Reality" notes and `reports/003-governance.md` §5.

## Pillars

| ID  | Pillar                    | Target                                                                                                                                                                                                                       | Verification                                                                                 |
| --- | ------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| P1  | Parity                    | 100% of P0 capabilities native at v1.0                                                                                                                                                                                       | `docs/parity/CAPABILITY_MATRIX.json` drift blocks CI (Prompt 009)                            |
| P2  | Security                  | Default-deny on every tool path; signed releases; 0 unfixed criticals > 72h; least-privilege CI tokens                                                                                                                       | Policy-broker suites; `scripts/sign-release.mjs` verify; audit age query; token scope review |
| P3  | Performance (CI-enforced) | ≤3s cold start p95; ≤100ms UI interaction; 60fps / ≤8ms GPU frame for 3D; ≤600MB idle RAM; ≤60MB installers; auto lite-mode                                                                                                  | CI perf gates on every push (method per metric below)                                        |
| P4  | Local-first               | Core fully functional offline with local models; telemetry opt-in only                                                                                                                                                       | Offline test pass with local models; telemetry default-off check                             |
| P5  | AAA UX                    | WCAG 2.1 AA; onboarding ≤10 min; Void Neon identity preserved                                                                                                                                                                | Accessibility audit; timed onboarding trial; identity review                                 |
| P6  | Evidence                  | Every side effect ledgered; replay UI; exportable run reports                                                                                                                                                                | `evidence_store` + `tool_receipt` chain tests; replay UI test; report export test            |
| P7  | Open governance           | Apache-2.0 core; public roadmap; weekly devlog; per-batch audit                                                                                                                                                              | License gate; roadmap/devlog presence; audit report per batch                                |
| P8  | Reliability               | ≥99.5% crash-free sessions; flake policy per `docs/CI.md` (max 1 rerun, tracked issues, `docs/evidence/FLAKES.md` register, quarantine ≤7 days); deterministic pool-shutdown stress on every push; 30-green CI streak target | Crash-free metric; required checks green; `stress-shutdown` job; streak count                |
| P9  | Cost transparency         | Pre-run estimates within ±10%; published unit costs                                                                                                                                                                          | Estimate-vs-actual test on the usage ledger; unit-cost doc presence                          |
| P10 | Charter                   | Every local feature free forever — moving any local capability behind a paywall is an AUTOMATIC ZERO in every audit. Paid = cloud/teams/managed only                                                                         | Audit charter check; pricing review against D7/D8                                            |

### P1 — Parity

Target: 100% of P0 capabilities native at v1.0; matrix drift blocks CI.
Verification: `docs/parity/CAPABILITY_MATRIX.json` (Prompt 009) diff gate.
Reality: matrix file does not yet exist; per-topic record is `docs/adr/0001–0012`.

### P2 — Security

Target: default-deny on every tool path; signed releases; 0 unfixed
criticals > 72h; least-privilege CI tokens.
Verification: `policy_broker` suites (default-deny is the ONLY side-effect
authority per `AGENTS.md` §2); release signing via `scripts/sign-release.mjs`
(minisign ed25519, updater pubkey in `tauri.conf.json`; private key via env,
never committed — `tools/sign-release.test.mjs` covers verify); vulnerability
age query; CI token scope review.
Reality: `NEVER access signing private keys` is a hard prohibition
(`AGENTS.md` §3); AI never touches them by standing order (D9).

### P3 — Performance (CI-enforced)

Target: ≤3s cold start p95; ≤100ms UI interaction; 60fps / ≤8ms GPU frame for
3D; ≤600MB idle RAM; ≤60MB installers; auto lite-mode.
Verification: CI perf gates measuring cold start, interaction latency, frame
time, idle RSS, installer bytes, with lite-mode fallback assertion.
Reality: perf baselines ride the `audit` job (`.talos/baselines.json`,
uploaded as `talos-audit-baselines` per `docs/CI.md`); P3 thresholds above
are targets to be encoded as gates, not current measurements.

### P4 — Local-first

Target: core fully functional offline with local models; telemetry opt-in only.
Verification: offline test pass (no network) with local models; telemetry
default-off check.
Reality: `crates/llm_gateway` ships an Ollama provider (`providers/ollama.rs`,
no key, `/api/tags` discovery) beside BYOK providers; ADR 0009 is the
local-first record.

### P5 — AAA UX

Target: WCAG 2.1 AA; onboarding ≤10 min; Void Neon identity preserved.
Verification: accessibility audit; timed onboarding trial; design identity review.
Reality: Void Neon identity lives in `apps/ui/src/features/void-neon/`
(plus `FpsMeter` for the 60fps side of P3).

### P6 — Evidence

Target: every side effect ledgered; replay UI; exportable run reports.
Verification: `evidence_store` + `tool_receipt` chain tests
(`crates/evidence_store`); replay UI test; report export test
(`reports/`, `docs/reports/audit-latest.{md,json}`).
Reality: evidence-first is a standing order (`AGENTS.md` §2, §7);
`crates/evidence_store` is the append-only receipt layer.

### P7 — Open governance

Target: Apache-2.0 core; public roadmap; weekly devlog; per-batch audit.
Verification: license gate; roadmap/devlog presence; one audit report per
batch in `reports/`.
Reality: repo is public (ADR 0005); Apache-2.0 core since Prompt 004
executed the relicense (D2, ADR 0013); roadmap/devlog cadence is
gated on Owner Task O-1 per D10.

### P8 — Reliability

Target: ≥99.5% crash-free sessions; flake policy per `docs/CI.md` (max 1 rerun,
tracked issues, `docs/evidence/FLAKES.md` register, quarantine ≤7 days);
deterministic pool-shutdown stress on every push; 30-green CI streak target.
Verification: crash-free metric; required checks green
(`setup/meta`, `typescript`, `rust-core`, `desktop-check`, `audit`,
`media-check`, plus convention-required `awakening`, `hardening`,
`stress-shutdown` per `docs/CI.md`); stress job output; streak count.
Reality: flake policy matches — `docs/CI.md` §"Flake policy (no silent
reruns)" enforces exactly max-1-rerun + tracked issue + FLAKES.md row
(deadline ≤7 days); `stress-shutdown` runs the 100-cycle pool-shutdown
stress on ubuntu + windows every push (report 002).

### P9 — Cost transparency

Target: pre-run estimates within ±10%; published unit costs.
Verification: estimate-vs-actual test on the usage ledger; unit-cost doc presence.
Reality: `usage_records` is the append-only spend source and per-task caps
(`runtime_tasks.max_spend_usd`) are enforced before any network traffic
(ADR 0006); the ±10% estimate-accuracy gate does not yet exist.

### P10 — Charter

Target: every local feature free forever — moving any local capability behind a
paywall is an AUTOMATIC ZERO in every audit. Paid = cloud/teams/managed only.
Verification: audit charter check; pricing review against D7/D8.
This pillar restates D7 and is non-negotiable.

## Cross-links

- Decisions: `docs/DECISIONS.md` (D1–D12, locked 2026-11-12).
- ADRs: `docs/adr/0001-*.md` through `0012-*.md`, plus
  `0013-apache2-relicense.md`, `0014-codename-public-brand.md`,
  `0015-byok-auth-fence.md`.
- Process: `docs/CI.md` (required checks, flake policy).
- Standing orders + per-prompt DoD: `AGENTS.md`, `CONTRIBUTING.md`.
