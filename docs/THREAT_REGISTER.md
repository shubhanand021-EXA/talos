# Talos Threat Register

Status: Accepted. Date: 2026-11-13. Owner: Shubh Anand.

This register names the competitive threats the parity matrix tracks against.
Competitor capability cells live ONLY in the sanctioned architect baseline
import (Prompt 013); nothing here asserts what a competitor ships today.
Each entry records the threat, the evidence date, our counter-move, and a
review cadence. Re-verify desktop/client claims before launch (D11).

## T1 — OpenClaw (velocity + foundation backing)

- **Threat:** Open-source agent moving fast with foundation/organizational
  backing; sets the pace for features and mindshare. Now ships a native
  Windows Hub plus a cross-platform Tauri desktop app (verified 2026-11-14).
- **Evidence date:** 2026-11-13 (architect competitive set; CVE-2026-25253
  and CVE-2026-33579 disclosed against the OpenClaw class during Prompt 006
  review — our daemon is hardened against both by construction);
  2026-11-14 (architect live verification of native Windows Hub + Tauri
  desktop app: docs.openclaw.ai/platforms/windows).
- **Counter-move:** Verified execution + unification + evidence — every claim
  names its test (evidence ledger, fidelity ladder), and the all-in-one
  desktop replaces the fragmented stack (VISION.md mission; P10 charter).
  Governance (default-deny broker, hash-chained receipts) is the moat a
  fast-moving UI cannot shortcut.
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
  distribution channel could out-distribute any independent desktop. Now
  ships an official desktop app (macOS Apple Silicon + Windows, Electron,
  MIT, dsh-0.2.2 merged 2026-11-13) — the "no desktop client" window is
  CLOSED.
- **Evidence date:** 2026-11-14 (architect live verification:
  deepseek.com/harness, github deepseek-ai/deepseek-harness apps/desktop;
  D11 premise overturned — re-verification complete).
- **Counter-move:** Verified execution + evidence ledger + default-deny
  governance + multi-provider BYOK neutrality + enterprise audit +
  Tauri-light performance — the wedge is no longer "ship the desktop
  first" but the trust + neutrality + efficiency stack a lab-owned
  Electron app cannot match.
- **Review cadence:** weekly (elevated while the harness ships fast),
  then quarterly.

## T4 — Platform-owner bundling (OS / IDE / suite vendors)

- **Threat:** OS, IDE, or productivity-suite vendors bundle "good enough"
  agents by default, raising distribution and default-placement barriers.
- **Evidence date:** 2026-11-13 (structural assessment; no single incident).
- **Counter-move:** Local-first sovereignty + open source — Apache-2.0 core,
  BYOK + local models, no account required, evidence you can audit.
  The uninstall test (VISION.md) is the standing defense review.
- **Review cadence:** quarterly, or on any major platform bundle announcement.

## Cadence log

| Date       | Action                                                                                                                                                    |
| ---------- | --------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 2026-11-13 | Register created (Prompt 009); all entries dated.                                                                                                         |
| 2026-11-14 | T1/T3 reset (Prompt 018): DeepSeek desktop SHIPPED (dsh-0.2.2), OpenClaw native Hub + Tauri app verified; counter-moves re-positioned; T3 cadence weekly. |
