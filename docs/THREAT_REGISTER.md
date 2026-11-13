# Talos Threat Register

Status: Accepted. Date: 2026-11-13. Owner: Shubh Anand.

This register names the competitive threats the parity matrix tracks against.
Competitor capability cells live ONLY in the sanctioned architect baseline
import (Prompt 013); nothing here asserts what a competitor ships today.
Each entry records the threat, the evidence date, our counter-move, and a
review cadence. Re-verify desktop/client claims before launch (D11).

## T1 — OpenClaw (velocity + foundation backing)

- **Threat:** Open-source agent moving fast with foundation/organizational
  backing; sets the pace for features and mindshare.
- **Evidence date:** 2026-11-13 (architect competitive set; CVE-2026-25253
  and CVE-2026-33579 disclosed against the OpenClaw class during Prompt 006
  review — our daemon is hardened against both by construction).
- **Counter-move:** Verified execution + unification — every claim names its
  test (evidence ledger, fidelity ladder), and the all-in-one desktop
  replaces the fragmented stack (VISION.md mission; P10 charter).
- **Review cadence:** quarterly, or on any OpenClaw security advisory.

## T2 — Hermes (learning loop + Nous Portal monetization)

- **Threat:** Closed learning loop (distill → recall → improve) paired with
  a monetized portal; network effects from captured workflows.
- **Evidence date:** 2026-11-13 (architect competitive set; GEPA skill loop
  recorded in `docs/adr/0012-gepa-skill-learning.md`).
- **Counter-move:** Evidence ledger + broader import — our GEPA loop is
  open-source and evidence-chained (`crates/skill_extractor`, usage-tracked
  recall), and the local-free charter (P10) undercuts portal lock-in.
- **Review cadence:** quarterly, alongside the D6 provider-policy review.

## T3 — DeepSeek Harness (8th primary competitor, D11)

- **Threat:** Model-lab-owned harness with explosive adoption; a model-lab
  distribution channel could out-distribute any independent desktop.
- **Evidence date:** 2026-11-12 (D11 locked); desktop status: NO official
  desktop client as of last verification — **re-verify before launch**.
- **Counter-move:** Ship the desktop command-center wedge first (Tauri 2 +
  React 19 + Void Neon, offline-first with local models); local-first
  sovereignty means no lab dependency for core use.
- **Review cadence:** before launch (mandatory re-verification), then quarterly.

## T4 — Platform-owner bundling (OS / IDE / suite vendors)

- **Threat:** OS, IDE, or productivity-suite vendors bundle "good enough"
  agents by default, raising distribution and default-placement barriers.
- **Evidence date:** 2026-11-13 (structural assessment; no single incident).
- **Counter-move:** Local-first sovereignty + open source — Apache-2.0 core,
  BYOK + local models, no account required, evidence you can audit.
  The uninstall test (VISION.md) is the standing defense review.
- **Review cadence:** quarterly, or on any major platform bundle announcement.

## Cadence log

| Date       | Action                                            |
| ---------- | ------------------------------------------------- |
| 2026-11-13 | Register created (Prompt 009); all entries dated. |
